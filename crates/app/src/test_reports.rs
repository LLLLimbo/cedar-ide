//! Read-only parsing of a bounded, single-`testsuite` Surefire XML subset.
//!
//! This is a historical report, never evidence that Cedar ran any tests. The
//! structure follows Maven's `surefire-test-report.xsd` (version 3.0.2), with
//! stricter semantic checks and explicit resource limits. It is not a general
//! JUnit XML reader or an XSD validator. XML 1.0 / UTF-8, unqualified elements,
//! and a single suite are supported. Schema-location attributes are inert.
//!
//! Declared tests/failures/errors/skipped are checked against testcase rows and
//! their ordinary outcome markers, counting repeated failures once per case.
//! Cases containing any rerun/flaky marker are visibly Unsupported, including
//! when they also contain a failure or error. Returned counts describe these
//! visible statuses and therefore need not equal the declared failure count.
//! An optional flakes count must equal cases containing flaky markers. Retry
//! attempt outcomes are not interpreted as a final pass. An empty suite has
//! zero passed cases. Mixed ordinary outcomes are rejected as ambiguous.
//!
//! Names and diagnostic details are inert strings, never paths or navigation
//! targets. Properties and all system output are validated and discarded.
//! Every error discards the entire result, including resource-limit failures.

use std::fmt;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

pub const MAX_REPORT_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 8;
const MAX_EVENTS: usize = 65_536;
const MAX_CASES: usize = 4096;
const MAX_PROPERTIES: usize = 2048;
const MAX_ATTRIBUTES: usize = 16;
const MAX_ATTRIBUTE_BYTES: usize = 8192;
const MAX_TOTAL_ATTRIBUTE_BYTES: usize = 256 * 1024;
const MAX_FIELD_BYTES: usize = 4096;
const MAX_DETAILS_PER_CASE: usize = 16;
const MAX_DETAIL_BYTES: usize = 16 * 1024;
const MAX_ELEMENT_TEXT_BYTES: usize = 64 * 1024;
const MAX_TOTAL_TEXT_BYTES: usize = 512 * 1024;
const MAX_RETAINED_BYTES: usize = 256 * 1024;
const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";

#[derive(Clone, Debug, PartialEq)]
pub struct TestReport {
    pub suite_name: String,
    pub counts: TestCounts,
    pub duration_seconds: Option<f64>,
    pub cases: Vec<TestCase>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestCounts {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub skipped: usize,
    pub unsupported: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TestCase {
    pub name: String,
    pub classname: Option<String>,
    pub status: TestStatus,
    pub duration_seconds: f64,
    pub details: Vec<TestDetail>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    Error,
    Skipped,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestDetail {
    pub kind: String,
    pub message: Option<String>,
    pub detail_type: Option<String>,
    pub text: String,
}

/// Diagnostics never echo attribute values, property values or captured output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportError {
    TooLarge,
    InvalidXml,
    Unsupported(&'static str),
    InvalidStructure(&'static str),
    InvalidValue(&'static str),
    LimitExceeded(&'static str),
    InconsistentCounts,
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("Report exceeds the 1 MiB input limit"),
            Self::InvalidXml => f.write_str("Report is not complete, well-formed XML 1.0"),
            Self::Unsupported(what) => write!(f, "Unsupported report: {what}"),
            Self::InvalidStructure(what) => write!(f, "Invalid report structure: {what}"),
            Self::InvalidValue(what) => write!(f, "Invalid report value: {what}"),
            Self::LimitExceeded(what) => write!(f, "Report exceeds the {what} limit"),
            Self::InconsistentCounts => {
                f.write_str("Report counts do not match its testcase outcome markers")
            }
        }
    }
}

impl std::error::Error for ReportError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Suite,
    Properties,
    Property,
    Case,
    Failure,
    Error,
    Skipped,
    RerunFailure,
    RerunError,
    FlakyFailure,
    FlakyError,
    StackTrace,
    Stdout,
    Stderr,
}

impl Kind {
    fn from_name(name: &[u8]) -> Result<Self, ReportError> {
        match name {
            b"testsuite" => Ok(Self::Suite),
            b"properties" => Ok(Self::Properties),
            b"property" => Ok(Self::Property),
            b"testcase" => Ok(Self::Case),
            b"failure" => Ok(Self::Failure),
            b"error" => Ok(Self::Error),
            b"skipped" => Ok(Self::Skipped),
            b"rerunFailure" => Ok(Self::RerunFailure),
            b"rerunError" => Ok(Self::RerunError),
            b"flakyFailure" => Ok(Self::FlakyFailure),
            b"flakyError" => Ok(Self::FlakyError),
            b"stackTrace" => Ok(Self::StackTrace),
            b"system-out" => Ok(Self::Stdout),
            b"system-err" => Ok(Self::Stderr),
            _ => Err(ReportError::Unsupported("element or namespace")),
        }
    }

    fn is_attempt(self) -> bool {
        matches!(
            self,
            Self::RerunFailure | Self::RerunError | Self::FlakyFailure | Self::FlakyError
        )
    }

    fn is_detail(self) -> bool {
        matches!(self, Self::Failure | Self::Error | Self::Skipped) || self.is_attempt()
    }

    fn is_text(self) -> bool {
        matches!(
            self,
            Self::Failure
                | Self::Error
                | Self::Skipped
                | Self::StackTrace
                | Self::Stdout
                | Self::Stderr
        )
    }

    fn bit(self) -> u32 {
        1 << self as u32
    }

    /// Return the schema sequence position and whether repetitions are allowed.
    fn child(self, child: Self) -> Option<(u8, bool)> {
        match (self, child) {
            (Self::Suite, Self::Properties) => Some((1, false)),
            (Self::Suite, Self::Case) | (Self::Properties, Self::Property) => Some((2, true)),
            (Self::Case, Self::Failure) => Some((1, true)),
            (Self::Case, Self::RerunFailure) => Some((2, true)),
            (Self::Case, Self::FlakyFailure) => Some((3, true)),
            (Self::Case, Self::Skipped) => Some((4, false)),
            (Self::Case, Self::Error) => Some((5, false)),
            (Self::Case, Self::RerunError) => Some((6, true)),
            (Self::Case, Self::FlakyError) => Some((7, true)),
            (Self::Case, Self::Stdout) => Some((8, false)),
            (Self::Case, Self::Stderr) => Some((9, false)),
            (parent, Self::StackTrace) if parent.is_attempt() => Some((1, false)),
            (parent, Self::Stdout) if parent.is_attempt() => Some((2, false)),
            (parent, Self::Stderr) if parent.is_attempt() => Some((3, false)),
            _ => None,
        }
    }

    fn permits_attribute(self, key: &str) -> bool {
        match self {
            Self::Suite => matches!(
                key,
                "name"
                    | "tests"
                    | "failures"
                    | "errors"
                    | "skipped"
                    | "flakes"
                    | "time"
                    | "version"
                    | "timestamp"
                    | "group"
                    | "xmlns"
                    | "xmlns:xsi"
                    | "xsi:noNamespaceSchemaLocation"
            ),
            Self::Case => matches!(key, "name" | "classname" | "time" | "group" | "timestamp"),
            Self::Property => matches!(key, "name" | "value"),
            Self::Failure | Self::Error => matches!(key, "message" | "type" | "xsi:nil"),
            Self::Skipped => matches!(key, "message" | "xsi:nil"),
            kind if kind.is_attempt() => matches!(key, "message" | "type"),
            _ => false,
        }
    }
}

struct Frame {
    kind: Kind,
    last_child: u8,
    children: u32,
    text_bytes: usize,
    detail: Option<usize>,
    nil: bool,
}

#[derive(Default)]
struct Budget {
    attributes: usize,
    text: usize,
    retained: usize,
}

impl Budget {
    fn retain(&mut self, len: usize) -> Result<(), ReportError> {
        self.retained += len;
        check_limit(self.retained, MAX_RETAINED_BYTES, "retained text")
    }

    fn field(&mut self, value: &str) -> Result<String, ReportError> {
        check_limit(value.len(), MAX_FIELD_BYTES, "name/message/type size")?;
        self.retain(value.len())?;
        Ok(value.to_owned())
    }

    fn text(&mut self, len: usize) -> Result<(), ReportError> {
        self.text += len;
        check_limit(self.text, MAX_TOTAL_TEXT_BYTES, "total text")
    }
}

#[derive(Default)]
struct DeclaredCounts {
    tests: usize,
    failures: usize,
    errors: usize,
    skipped: usize,
    flakes: Option<usize>,
}

struct Parser {
    frames: Vec<Frame>,
    report: Option<TestReport>,
    current_case: Option<TestCase>,
    declared: DeclaredCounts,
    observed: DeclaredCounts,
    budget: Budget,
    properties: usize,
    xsi: bool,
    finished: bool,
}

/// Parse one complete UTF-8 XML report. No filesystem, process or network access.
pub fn parse_report(input: &str) -> Result<TestReport, ReportError> {
    if input.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge);
    }
    if !input.chars().all(xml_char) {
        return Err(ReportError::InvalidXml);
    }
    let mut reader = Reader::from_str(input);
    reader.config_mut().check_comments = true;
    reader.config_mut().check_end_names = true;
    let mut parser = Parser {
        frames: Vec::new(),
        report: None,
        current_case: None,
        declared: DeclaredCounts::default(),
        observed: DeclaredCounts::default(),
        budget: Budget::default(),
        properties: 0,
        xsi: false,
        finished: false,
    };
    let mut events = 0;
    loop {
        events += 1;
        check_limit(events, MAX_EVENTS, "XML event count")?;
        match reader.read_event().map_err(|_| ReportError::InvalidXml)? {
            Event::Start(start) => parser.start(&start, &reader)?,
            Event::Empty(start) => {
                parser.start(&start, &reader)?;
                parser.end(Kind::from_name(start.name().as_ref())?)?;
            }
            Event::End(end) => parser.end(Kind::from_name(end.name().as_ref())?)?,
            Event::Text(text) => {
                if text.as_ref().windows(3).any(|bytes| bytes == b"]]>") {
                    return Err(ReportError::InvalidXml);
                }
                let text = text.xml10_content().map_err(|_| ReportError::InvalidXml)?;
                parser.text(&text, false)?;
            }
            Event::CData(text) => {
                if !parser
                    .frames
                    .last()
                    .is_some_and(|frame| frame.kind.is_text())
                {
                    return Err(ReportError::InvalidStructure(
                        "CDATA outside a text element",
                    ));
                }
                let text = text.xml10_content().map_err(|_| ReportError::InvalidXml)?;
                parser.text(&text, false)?;
            }
            Event::GeneralRef(reference) => {
                // A character reference is not whitespace outside the root.
                if parser.frames.is_empty() {
                    return Err(ReportError::InvalidXml);
                }
                let name =
                    std::str::from_utf8(reference.as_ref()).map_err(|_| ReportError::InvalidXml)?;
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|_| ReportError::InvalidXml)?
                {
                    if !xml_char(character) {
                        return Err(ReportError::InvalidXml);
                    }
                    parser.text(character.encode_utf8(&mut [0; 4]), false)?;
                } else {
                    let text = quick_xml::escape::resolve_xml_entity(name)
                        .ok_or(ReportError::Unsupported("entity reference"))?;
                    parser.text(text, false)?;
                }
            }
            Event::Comment(comment) => {
                parser.text(
                    std::str::from_utf8(comment.as_ref()).map_err(|_| ReportError::InvalidXml)?,
                    true,
                )?;
            }
            Event::Decl(declaration) => {
                if events != 1 {
                    return Err(ReportError::InvalidXml);
                }
                validate_declaration(declaration.as_ref(), &reader, &mut parser.budget)?;
            }
            Event::DocType(_) => return Err(ReportError::Unsupported("DTD declarations")),
            Event::PI(_) => return Err(ReportError::Unsupported("processing instructions")),
            Event::Eof => break,
        }
    }
    if !parser.finished || !parser.frames.is_empty() || parser.current_case.is_some() {
        return Err(ReportError::InvalidXml);
    }
    if parser.declared.tests != parser.observed.tests
        || parser.declared.failures != parser.observed.failures
        || parser.declared.errors != parser.observed.errors
        || parser.declared.skipped != parser.observed.skipped
        || parser
            .declared
            .flakes
            .is_some_and(|flakes| flakes != parser.observed.flakes.unwrap_or(0))
    {
        return Err(ReportError::InconsistentCounts);
    }
    parser.report.ok_or(ReportError::InvalidXml)
}

impl Parser {
    fn start(&mut self, start: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<(), ReportError> {
        check_limit(self.frames.len() + 1, MAX_DEPTH, "XML depth")?;
        let kind = Kind::from_name(start.name().as_ref())?;
        if let Some(parent) = self.frames.last_mut() {
            let (position, repeat) = parent
                .kind
                .child(kind)
                .ok_or(ReportError::InvalidStructure("element nesting"))?;
            if position < parent.last_child || (!repeat && parent.children & kind.bit() != 0) {
                return Err(ReportError::InvalidStructure("element order or repetition"));
            }
            parent.last_child = position;
            parent.children |= kind.bit();
        } else if kind != Kind::Suite || self.report.is_some() || self.finished {
            return Err(ReportError::InvalidStructure("expected one testsuite root"));
        }
        let attributes = read_attributes(start, reader, &mut self.budget)?;
        if attributes
            .iter()
            .any(|(key, _)| !kind.permits_attribute(key))
        {
            return Err(ReportError::Unsupported("attribute or namespace"));
        }
        if let Some(timestamp) = attr(&attributes, "timestamp") {
            validate_timestamp(timestamp)?;
        }
        let mut detail = None;
        let mut nil = false;
        match kind {
            Kind::Suite => {
                if attr(&attributes, "xmlns").is_some_and(|namespace| !namespace.is_empty()) {
                    return Err(ReportError::Unsupported("default namespace"));
                }
                if let Some(namespace) = attr(&attributes, "xmlns:xsi") {
                    if namespace != XSI_NAMESPACE {
                        return Err(ReportError::Unsupported("schema-instance namespace"));
                    }
                    self.xsi = true;
                }
                if attr(&attributes, "xsi:noNamespaceSchemaLocation").is_some() && !self.xsi {
                    return Err(ReportError::InvalidXml);
                }
                self.declared = DeclaredCounts {
                    tests: parse_count(required(&attributes, "tests")?)?,
                    failures: parse_count(required(&attributes, "failures")?)?,
                    errors: parse_count(required(&attributes, "errors")?)?,
                    skipped: parse_count(required(&attributes, "skipped")?)?,
                    flakes: attr(&attributes, "flakes").map(parse_count).transpose()?,
                };
                if self.declared.failures + self.declared.errors + self.declared.skipped
                    > self.declared.tests
                {
                    return Err(ReportError::InconsistentCounts);
                }
                self.report = Some(TestReport {
                    suite_name: self.budget.field(required_name(&attributes)?)?,
                    counts: TestCounts::default(),
                    duration_seconds: attr(&attributes, "time").map(parse_duration).transpose()?,
                    cases: Vec::new(),
                });
            }
            Kind::Case => {
                check_limit(self.observed.tests + 1, MAX_CASES, "testcase count")?;
                self.current_case = Some(TestCase {
                    name: self.budget.field(required_name(&attributes)?)?,
                    classname: attr(&attributes, "classname")
                        .map(|value| self.budget.field(value))
                        .transpose()?,
                    duration_seconds: parse_duration(required(&attributes, "time")?)?,
                    status: TestStatus::Passed,
                    details: Vec::new(),
                });
            }
            Kind::Property => {
                self.properties += 1;
                check_limit(self.properties, MAX_PROPERTIES, "property count")?;
                required(&attributes, "name")?;
                required(&attributes, "value")?;
            }
            child if child.is_detail() => {
                if let Some(value) = attr(&attributes, "xsi:nil") {
                    if !self.xsi {
                        return Err(ReportError::InvalidXml);
                    }
                    nil = match value {
                        "true" | "1" => true,
                        "false" | "0" => false,
                        _ => return Err(ReportError::InvalidValue("xsi:nil must be boolean")),
                    };
                }
                let case = self.current_case.as_mut().ok_or(ReportError::InvalidXml)?;
                check_limit(
                    case.details.len() + 1,
                    MAX_DETAILS_PER_CASE,
                    "details per testcase",
                )?;
                detail = Some(case.details.len());
                case.details.push(TestDetail {
                    kind: self.budget.field(
                        std::str::from_utf8(start.name().as_ref())
                            .map_err(|_| ReportError::InvalidXml)?,
                    )?,
                    message: attr(&attributes, "message")
                        .map(|value| self.budget.field(value))
                        .transpose()?,
                    detail_type: attr(&attributes, "type")
                        .map(|value| self.budget.field(value))
                        .transpose()?,
                    text: String::new(),
                });
            }
            Kind::StackTrace => {
                detail = self.frames.last().and_then(|parent| parent.detail);
            }
            _ => {}
        }
        self.frames.push(Frame {
            kind,
            last_child: 0,
            children: 0,
            text_bytes: 0,
            detail,
            nil,
        });
        Ok(())
    }

    fn end(&mut self, kind: Kind) -> Result<(), ReportError> {
        let frame = self.frames.pop().ok_or(ReportError::InvalidXml)?;
        if frame.kind != kind {
            return Err(ReportError::InvalidXml);
        }
        if kind.is_attempt() && frame.children & Kind::StackTrace.bit() == 0 {
            return Err(ReportError::InvalidStructure(
                "retry/flaky detail requires stackTrace",
            ));
        }
        if kind == Kind::Case {
            let failure = frame.children & Kind::Failure.bit() != 0;
            let error = frame.children & Kind::Error.bit() != 0;
            let skipped = frame.children & Kind::Skipped.bit() != 0;
            if usize::from(failure) + usize::from(error) + usize::from(skipped) > 1 {
                return Err(ReportError::InvalidStructure(
                    "conflicting testcase outcomes",
                ));
            }
            let flaky = frame.children & (Kind::FlakyFailure.bit() | Kind::FlakyError.bit()) != 0;
            let unsupported =
                flaky || frame.children & (Kind::RerunFailure.bit() | Kind::RerunError.bit()) != 0;
            self.observed.tests += 1;
            self.observed.failures += usize::from(failure);
            self.observed.errors += usize::from(error);
            self.observed.skipped += usize::from(skipped);
            self.observed.flakes = Some(self.observed.flakes.unwrap_or(0) + usize::from(flaky));
            let mut case = self.current_case.take().ok_or(ReportError::InvalidXml)?;
            let report = self.report.as_mut().ok_or(ReportError::InvalidXml)?;
            report.counts.total += 1;
            case.status = if unsupported {
                report.counts.unsupported += 1;
                TestStatus::Unsupported
            } else if failure {
                report.counts.failed += 1;
                TestStatus::Failed
            } else if error {
                report.counts.errors += 1;
                TestStatus::Error
            } else if skipped {
                report.counts.skipped += 1;
                TestStatus::Skipped
            } else {
                report.counts.passed += 1;
                TestStatus::Passed
            };
            report.cases.push(case);
        } else if kind == Kind::Suite {
            self.finished = true;
        }
        Ok(())
    }

    fn text(&mut self, text: &str, comment: bool) -> Result<(), ReportError> {
        check_limit(text.len(), MAX_ELEMENT_TEXT_BYTES, "element text")?;
        self.budget.text(text.len())?;
        if comment {
            return Ok(());
        }
        let Some(frame) = self.frames.last_mut() else {
            return if xml_whitespace(text) {
                Ok(())
            } else {
                Err(ReportError::InvalidXml)
            };
        };
        frame.text_bytes += text.len();
        check_limit(frame.text_bytes, MAX_ELEMENT_TEXT_BYTES, "element text")?;
        if frame.nil && !text.is_empty() {
            return Err(ReportError::InvalidStructure("nil detail contains text"));
        }
        if !frame.kind.is_text() {
            if !xml_whitespace(text) {
                return Err(ReportError::InvalidStructure("text outside a text element"));
            }
            return Ok(());
        }
        if let Some(index) = frame.detail {
            let detail = self
                .current_case
                .as_mut()
                .and_then(|case| case.details.get_mut(index))
                .ok_or(ReportError::InvalidXml)?;
            check_limit(
                detail.text.len() + text.len(),
                MAX_DETAIL_BYTES,
                "diagnostic detail size",
            )?;
            self.budget.retain(text.len())?;
            detail.text.push_str(text);
        }
        Ok(())
    }
}

type Attributes = Vec<(String, String)>;

fn read_attributes(
    start: &BytesStart<'_>,
    reader: &Reader<&[u8]>,
    budget: &mut Budget,
) -> Result<Attributes, ReportError> {
    validate_attribute_separators(start.attributes_raw())?;
    let mut values = Vec::new();
    for attribute in start.attributes() {
        check_limit(values.len() + 1, MAX_ATTRIBUTES, "attributes per element")?;
        let attribute = attribute.map_err(|_| ReportError::InvalidXml)?;
        check_limit(
            attribute.key.as_ref().len(),
            MAX_FIELD_BYTES,
            "attribute name size",
        )?;
        check_limit(
            attribute.value.len(),
            MAX_ATTRIBUTE_BYTES,
            "attribute value size",
        )?;
        budget.attributes += attribute.key.as_ref().len() + attribute.value.len();
        check_limit(
            budget.attributes,
            MAX_TOTAL_ATTRIBUTE_BYTES,
            "total attribute size",
        )?;
        if attribute.value.contains(&b'<') {
            return Err(ReportError::InvalidXml);
        }
        let value = attribute
            .decoded_and_normalized_value_with(
                XmlVersion::Explicit1_0,
                reader.decoder(),
                1,
                quick_xml::escape::resolve_xml_entity,
            )
            .map_err(|_| ReportError::InvalidXml)?;
        if !value.chars().all(xml_char) {
            return Err(ReportError::InvalidXml);
        }
        let name =
            std::str::from_utf8(attribute.key.as_ref()).map_err(|_| ReportError::InvalidXml)?;
        values.push((name.to_owned(), value.into_owned()));
    }
    Ok(values)
}

/// quick-xml's attribute iterator accepts adjacent quoted attributes. XML
/// requires whitespace before each one, including declaration attributes.
fn validate_attribute_separators(bytes: &[u8]) -> Result<(), ReportError> {
    let mut index = 0;
    while index < bytes.len() {
        let before_space = index;
        while bytes.get(index).is_some_and(|byte| xml_space(*byte)) {
            index += 1;
        }
        if index == bytes.len() {
            return Ok(());
        }
        if index == before_space {
            return Err(ReportError::InvalidXml);
        }
        let name_start = index;
        while bytes
            .get(index)
            .is_some_and(|byte| !xml_space(*byte) && *byte != b'=')
        {
            index += 1;
        }
        if index == name_start {
            return Err(ReportError::InvalidXml);
        }
        while bytes.get(index).is_some_and(|byte| xml_space(*byte)) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            return Err(ReportError::InvalidXml);
        }
        index += 1;
        while bytes.get(index).is_some_and(|byte| xml_space(*byte)) {
            index += 1;
        }
        let quote = *bytes.get(index).ok_or(ReportError::InvalidXml)?;
        if !matches!(quote, b'\'' | b'"') {
            return Err(ReportError::InvalidXml);
        }
        index += 1;
        while bytes.get(index).is_some_and(|byte| *byte != quote) {
            index += 1;
        }
        if index == bytes.len() {
            return Err(ReportError::InvalidXml);
        }
        index += 1;
    }
    Ok(())
}

fn validate_declaration(
    bytes: &[u8],
    reader: &Reader<&[u8]>,
    budget: &mut Budget,
) -> Result<(), ReportError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ReportError::InvalidXml)?;
    if !text.starts_with("xml") || text.contains('&') {
        return Err(ReportError::InvalidXml);
    }
    let start = BytesStart::from_content(text, 3);
    let attributes = read_attributes(&start, reader, budget)?;
    if attributes
        .first()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        != Some(("version", "1.0"))
    {
        return Err(ReportError::Unsupported("XML declaration version"));
    }
    let mut last = 0;
    for (key, value) in attributes.iter().skip(1) {
        let order = match key.as_str() {
            "encoding" if value.eq_ignore_ascii_case("utf-8") => 1,
            "standalone" if matches!(value.as_str(), "yes" | "no") => 2,
            _ => return Err(ReportError::Unsupported("XML declaration attribute")),
        };
        if order <= last {
            return Err(ReportError::InvalidXml);
        }
        last = order;
    }
    Ok(())
}

fn attr<'a>(attributes: &'a Attributes, key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn required<'a>(attributes: &'a Attributes, key: &str) -> Result<&'a str, ReportError> {
    attr(attributes, key).ok_or(ReportError::InvalidStructure("required attribute missing"))
}

fn required_name(attributes: &Attributes) -> Result<&str, ReportError> {
    let name = required(attributes, "name")?;
    if name.trim().is_empty() {
        return Err(ReportError::InvalidValue("name must not be empty"));
    }
    Ok(name)
}

fn parse_count(value: &str) -> Result<usize, ReportError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ReportError::InvalidValue(
            "counts must be unsigned decimal integers",
        ));
    }
    let value = value
        .parse()
        .map_err(|_| ReportError::InvalidValue("count overflow"))?;
    check_limit(value, MAX_CASES, "declared testcase count")?;
    Ok(value)
}

fn parse_duration(value: &str) -> Result<f64, ReportError> {
    // Rust floats include non-XML spellings such as `inf`, so check the lexical
    // alphabet and finiteness separately. No trimming or negative signed zero.
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b".+-eE".contains(&byte))
    {
        return Err(ReportError::InvalidValue(
            "duration must be finite and nonnegative",
        ));
    }
    let seconds: f64 = value
        .parse()
        .map_err(|_| ReportError::InvalidValue("duration"))?;
    if !seconds.is_finite() || seconds.is_sign_negative() {
        return Err(ReportError::InvalidValue(
            "duration must be finite and nonnegative",
        ));
    }
    Ok(seconds)
}

/// Accepted timestamp subset: YYYY-MM-DDThh:mm:ss[.fraction][Z|+hh:mm|-hh:mm].
/// It is validated and discarded, never used to claim report freshness.
fn validate_timestamp(value: &str) -> Result<(), ReportError> {
    let invalid = ReportError::InvalidValue("timestamp");
    let bytes = value.as_bytes();
    if bytes.len() < 19
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return Err(invalid);
    }
    let number = |start: usize, end: usize| -> Result<u32, ReportError> {
        let part = &bytes[start..end];
        if !part.iter().all(u8::is_ascii_digit) {
            return Err(ReportError::InvalidValue("timestamp"));
        }
        Ok(part
            .iter()
            .fold(0, |value, byte| value * 10 + u32::from(byte - b'0')))
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid),
    };
    if year == 0
        || day == 0
        || day > days
        || number(11, 13)? > 23
        || number(14, 16)? > 59
        || number(17, 19)? > 59
    {
        return Err(invalid);
    }
    let mut end = 19;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let first = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if first == end {
            return Err(invalid);
        }
    }
    match &bytes[end..] {
        [] | [b'Z'] => Ok(()),
        [b'+' | b'-', _, _, b':', _, _] => {
            let hours = number(end + 1, end + 3)?;
            let minutes = number(end + 4, end + 6)?;
            if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
                Err(invalid)
            } else {
                Ok(())
            }
        }
        _ => Err(invalid),
    }
}

fn check_limit(value: usize, maximum: usize, name: &'static str) -> Result<(), ReportError> {
    if value > maximum {
        Err(ReportError::LimitExceeded(name))
    } else {
        Ok(())
    }
}

fn xml_char(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn xml_whitespace(text: &str) -> bool {
    text.bytes().all(xml_space)
}

fn xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite(body: &str, tests: usize, failures: usize, errors: usize, skipped: usize) -> String {
        format!(
            "<testsuite name=\"Suite\" tests=\"{tests}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"{skipped}\">{body}</testsuite>"
        )
    }

    fn case(body: &str) -> String {
        format!("<testcase name=\"test\" classname=\"pkg.Tests\" time=\"0.125\">{body}</testcase>")
    }

    #[test]
    fn parses_the_provenanced_upstream_surefire_fixture() {
        // Compatibility with an upstream XML fixture, not a local producer run.
        let report = parse_report(include_str!(
            "../tests/fixtures/surefire-3.5.4/enclosed-error.xml"
        ))
        .unwrap();
        assert_eq!(report.suite_name, "surefire.MyTest");
        assert_eq!(report.duration_seconds, Some(0.0));
        assert_eq!(report.counts.total, 1);
        assert_eq!(report.counts.errors, 1);
        assert_eq!(report.counts.passed, 0);
        assert_eq!(report.cases[0].name, "t");
        assert_eq!(
            report.cases[0].classname.as_deref(),
            Some("surefire.MyTest$A")
        );
        assert_eq!(report.cases[0].status, TestStatus::Error);
        assert_eq!(report.cases[0].duration_seconds, 0.0);
        assert!(report.cases[0].details[0]
            .text
            .contains("surefire.MyTest$A.t"));
        let debug = format!("{report:?}");
        assert!(!debug.contains("java.runtime.name"));
        assert!(!debug.contains("Java(TM) SE Runtime Environment"));
    }

    #[test]
    fn extracts_all_ordinary_outcomes_and_counts_repeated_failures_once() {
        let xml = suite(
            &[
                case(""),
                case("<failure message=\"expected\" type=\"AssertionError\">one</failure><failure>two</failure>"),
                case("<error type=\"Exception\">trace</error>"),
                case("<skipped message=\"disabled\"/>"),
            ].concat(),
            4, 1, 1, 1,
        );
        let report = parse_report(&xml).unwrap();
        assert_eq!(report.suite_name, "Suite");
        assert_eq!(report.duration_seconds, None);
        assert_eq!(
            report.counts,
            TestCounts {
                total: 4,
                passed: 1,
                failed: 1,
                errors: 1,
                skipped: 1,
                unsupported: 0
            }
        );
        assert_eq!(report.cases[1].details.len(), 2);
        assert_eq!(
            report.cases[1].details[0].message.as_deref(),
            Some("expected")
        );
        assert_eq!(
            report.cases[1].details[0].detail_type.as_deref(),
            Some("AssertionError")
        );
        assert_eq!(report.cases[1].details[0].text, "one");
        assert_eq!(report.cases[0].duration_seconds, 0.125);
    }

    #[test]
    fn decodes_unicode_entities_cdata_and_xml_line_endings() {
        let xml = "\u{feff}<?xml version='1.0' encoding='UTF-8' standalone='yes'?><testsuite name='中文&amp;&#x1F642;' tests='1' failures='1' errors='0' skipped='0' time='1e-3'><testcase name='na&#109;e' classname='包.类' time='.25'><failure message='&quot;&lt;&amp;&apos;&gt;' type='断言'>中文 &amp; &#128578;<![CDATA[<inert>\r\nline]]>\rnext</failure></testcase></testsuite>";
        let report = parse_report(xml).unwrap();
        assert_eq!(report.suite_name, "中文&🙂");
        assert_eq!(report.duration_seconds, Some(0.001));
        assert_eq!(report.cases[0].name, "name");
        assert_eq!(report.cases[0].classname.as_deref(), Some("包.类"));
        assert_eq!(
            report.cases[0].details[0].message.as_deref(),
            Some("\"<&'>")
        );
        assert_eq!(
            report.cases[0].details[0].text,
            "中文 & 🙂<inert>\nline\nnext"
        );
    }

    #[test]
    fn properties_and_output_are_validated_but_never_retained() {
        let xml = suite(
            &format!("<properties><property name='password' value='private-property'/></properties>{}", case("<system-out>private-output &amp; text</system-out><system-err><![CDATA[private-error]]></system-err>")),
            1, 0, 0, 0,
        );
        let report = parse_report(&xml).unwrap();
        let debug = format!("{report:?}");
        for private in [
            "password",
            "private-property",
            "private-output",
            "private-error",
        ] {
            assert!(!debug.contains(private));
        }
        assert_eq!(report.cases[0].status, TestStatus::Passed);
        assert!(report.cases[0].details.is_empty());
        let bad = xml.replace("private-property", "&external;");
        assert!(parse_report(&bad).is_err());
        let error = parse_report(&xml.replace("private-output &amp; text", "&private-secret;"))
            .unwrap_err();
        assert!(!error.to_string().contains("private-secret"));
    }

    #[test]
    fn retry_and_flaky_variants_are_never_passed() {
        for kind in ["rerunFailure", "rerunError", "flakyFailure", "flakyError"] {
            let body = format!("<{kind} message='attempt'><stackTrace>trace</stackTrace><system-out>discard</system-out><system-err>discard</system-err></{kind}>");
            let report = parse_report(&suite(&case(&body), 1, 0, 0, 0)).unwrap();
            assert_eq!(report.cases[0].status, TestStatus::Unsupported, "{kind}");
            assert_eq!(report.counts.unsupported, 1);
            assert_eq!(report.counts.passed, 0);
            assert_eq!(report.cases[0].details[0].text, "trace");
        }
        let body = case("<failure>original</failure><rerunFailure><stackTrace>retry</stackTrace></rerunFailure>");
        let report = parse_report(&suite(&body, 1, 1, 0, 0)).unwrap();
        assert_eq!(report.counts.failed, 0);
        assert_eq!(report.counts.unsupported, 1);
        assert_eq!(
            parse_report(&suite(&body, 1, 0, 0, 0)),
            Err(ReportError::InconsistentCounts)
        );
    }

    #[test]
    fn validates_flakes_instead_of_treating_unrepresented_flakes_as_passes() {
        let ordinary =
            suite(&case(""), 1, 0, 0, 0).replacen("<testsuite ", "<testsuite flakes='1' ", 1);
        assert_eq!(
            parse_report(&ordinary),
            Err(ReportError::InconsistentCounts)
        );
        let flaky = suite(
            &case("<flakyFailure><stackTrace/></flakyFailure>"),
            1,
            0,
            0,
            0,
        )
        .replacen("<testsuite ", "<testsuite flakes='1' ", 1);
        assert_eq!(parse_report(&flaky).unwrap().counts.unsupported, 1);
        assert_eq!(
            parse_report(&flaky.replace("flakes='1'", "flakes='0'")),
            Err(ReportError::InconsistentCounts)
        );
    }

    #[test]
    fn zero_test_suite_is_empty_without_inventing_passes() {
        let report =
            parse_report("<testsuite name='empty' tests='0' failures='0' errors='0' skipped='0'/>")
                .unwrap();
        assert!(report.cases.is_empty());
        assert_eq!(report.counts, TestCounts::default());
    }

    #[test]
    fn rejects_inconsistent_or_ambiguous_counts() {
        for xml in [
            suite(&case(""), 2, 0, 0, 0),
            suite(&case("<failure/>"), 1, 0, 0, 0),
            suite(&case(""), 1, 1, 0, 0),
            suite(&case("<skipped/>"), 1, 0, 1, 0),
            suite(&case("<failure/><skipped/>"), 1, 1, 0, 1),
            suite(&case("<failure/><error/>"), 1, 1, 0, 0),
        ] {
            assert!(parse_report(&xml).is_err(), "{xml}");
        }
        for bad in [
            "-1",
            "+1",
            "1.0",
            "1e0",
            " 1",
            "NaN",
            "184467440737095516160",
        ] {
            let xml =
                suite(&case(""), 1, 0, 0, 0).replace("tests=\"1\"", &format!("tests=\"{bad}\""));
            assert!(parse_report(&xml).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_invalid_duration_on_suite_and_case() {
        for bad in [
            "-0", "-0.1", "NaN", "INF", "inf", "-inf", "1e999", "", " 1", "1 ", "0x10", "1,2",
        ] {
            let xml =
                suite(&case(""), 1, 0, 0, 0).replace("time=\"0.125\"", &format!("time=\"{bad}\""));
            assert!(parse_report(&xml).is_err(), "case {bad}");
            let xml = suite("", 0, 0, 0, 0).replacen(
                "<testsuite ",
                &format!("<testsuite time=\"{bad}\" "),
                1,
            );
            assert!(parse_report(&xml).is_err(), "suite {bad}");
        }
    }

    #[test]
    fn rejects_dtd_external_entities_and_invalid_scalar_references() {
        let valid = suite(&case(""), 1, 0, 0, 0);
        for prefix in [
            "<!DOCTYPE testsuite SYSTEM 'file:///private'>",
            "<!DOCTYPE testsuite [<!ENTITY x SYSTEM 'https://example.invalid/secret'>]>",
            "<!DOCTYPE testsuite [<!ENTITY a 'value'><!ENTITY b '&a;&a;'>]>",
            "<?xml-stylesheet href='https://example.invalid/style'?>",
        ] {
            assert!(parse_report(&format!("{prefix}{valid}")).is_err());
        }
        for bad in [
            "&unknown;",
            "&",
            "&#0;",
            "&#x1;",
            "&#xD800;",
            "&#xFFFE;",
            "&#x110000;",
            "\u{1}",
        ] {
            assert!(
                parse_report(&suite(
                    &case(&format!("<failure>{bad}</failure>")),
                    1,
                    1,
                    0,
                    0
                ))
                .is_err(),
                "text {bad}"
            );
            assert!(
                parse_report(&valid.replace("name=\"test\"", &format!("name=\"{bad}\""))).is_err(),
                "attribute {bad}"
            );
        }
    }

    #[test]
    fn rejects_partial_multiple_root_and_malformed_xml() {
        let valid = suite(&case(""), 1, 0, 0, 0);
        for xml in [
            String::new(),
            "  ".to_owned(),
            valid.trim_end_matches("</testsuite>").to_owned(),
            format!("{valid}{valid}"),
            format!("\u{feff}\u{feff}{valid}"),
            format!("text{valid}"),
            format!("{valid}text"),
            format!("&#32;{valid}"),
            format!("<![CDATA[ ]]>{valid}"),
            valid.replace("</testcase>", "</wrong>"),
            valid.replace("name=\"test\"", "name='first' name='second'"),
            valid.replace("name=\"Suite\" tests", "name=\"Suite\"tests"),
            valid.replace("name=\"test\"", "name=unquoted"),
            valid.replace("name=\"test\"", "name='a<b'"),
            valid.replace("</testcase>", "]]></testcase>"),
            format!("<!-- illegal -- comment -->{valid}"),
        ] {
            assert!(parse_report(&xml).is_err(), "{xml}");
        }
    }

    #[test]
    fn rejects_wrong_nesting_order_unknown_elements_and_missing_attributes() {
        for body in [
            "<testcase name='x' time='0'><testcase name='y' time='0'/></testcase>",
            "<testcase name='x' time='0'><failure><nested/></failure></testcase>",
            "<testcase name='x' time='0'><system-out/><failure/></testcase>",
            "<testcase name='x' time='0'><error/><error/></testcase>",
            "<testcase name='x' time='0'><skipped/><skipped/></testcase>",
            "<testcase name='x' time='0'><rerunFailure/></testcase>",
            "<testcase name='x' time='0'><flakyError><system-out/><stackTrace/></flakyError></testcase>",
            "<testcase name='x' time='0' file='guessed.java'/>",
            "<testcase name='x'/>",
            "<testcase time='0'/>",
            "<testcase name='x' time='0'/><properties/>",
            "<properties><property name='x'/></properties><testcase name='x' time='0'/>",
            "<testsuites/>",
            "<system-out>suite output is outside this subset</system-out>",
        ] {
            assert!(parse_report(&suite(body, 1, 0, 0, 0)).is_err(), "{body}");
        }
        assert!(parse_report("<testsuite name='x' tests='0'/>").is_err());
    }

    #[test]
    fn declaration_and_namespace_metadata_are_strict_and_inert() {
        let valid = suite(&case("<failure xsi:nil='true'/>"), 1, 1, 0, 0)
            .replacen("<testsuite ", &format!("<testsuite xmlns:xsi='{XSI_NAMESPACE}' xsi:noNamespaceSchemaLocation='https://example.invalid/never-fetch.xsd' "), 1);
        assert!(parse_report(&valid).is_ok());
        assert!(
            parse_report(&valid.replace("xsi:nil='true'/>", "xsi:nil='true'>text</failure>"))
                .is_err()
        );
        assert!(parse_report(&valid.replace("xsi:nil='true'", "xsi:nil='perhaps'")).is_err());
        assert!(
            parse_report(&valid.replace(XSI_NAMESPACE, "https://example.invalid/namespace"))
                .is_err()
        );
        assert!(parse_report(&suite(&case("<failure xsi:nil='true'/>"), 1, 1, 0, 0)).is_err());
        let empty = suite("", 0, 0, 0, 0);
        assert!(
            parse_report(&empty.replacen("<testsuite ", "<testsuite xmlns='urn:other' ", 1))
                .is_err()
        );
        for declaration in [
            " <?xml version='1.0'?>",
            "<!--before--><?xml version='1.0'?>",
            "<?xml version='1.1'?>",
            "<?xml version='1.0' version='1.0'?>",
            "<?xml version='1.0'encoding='UTF-8'?>",
            "<?xml encoding='UTF-8' version='1.0'?>",
            "<?xml version='1.0' standalone='yes' encoding='UTF-8'?>",
            "<?xml version='1.0' encoding='UTF-16'?>",
            "<?xml version='1.0' standalone='maybe'?>",
        ] {
            assert!(
                parse_report(&format!("{declaration}{empty}")).is_err(),
                "{declaration}"
            );
        }
    }

    #[test]
    fn input_and_retained_detail_limits_fail_the_whole_report() {
        assert_eq!(
            parse_report(&" ".repeat(MAX_REPORT_BYTES + 1)),
            Err(ReportError::TooLarge)
        );
        let exact = suite(
            &case(&format!(
                "<failure>{}</failure>",
                "x".repeat(MAX_DETAIL_BYTES)
            )),
            1,
            1,
            0,
            0,
        );
        assert!(parse_report(&exact).is_ok());
        let over = suite(
            &case(&format!(
                "<failure>{}</failure>",
                "x".repeat(MAX_DETAIL_BYTES + 1)
            )),
            1,
            1,
            0,
            0,
        );
        assert_eq!(
            parse_report(&over),
            Err(ReportError::LimitExceeded("diagnostic detail size"))
        );
        let unicode = suite(
            &case(&format!(
                "<failure>{}</failure>",
                "中".repeat(MAX_DETAIL_BYTES / 3 + 1)
            )),
            1,
            1,
            0,
            0,
        );
        assert!(parse_report(&unicode).is_err());
        let split = suite(
            &case(&format!(
                "<failure>{}<![CDATA[x]]></failure>",
                "x".repeat(MAX_DETAIL_BYTES)
            )),
            1,
            1,
            0,
            0,
        );
        assert!(parse_report(&split).is_err());
        let fields = suite(&case(""), 1, 0, 0, 0).replace(
            "name=\"test\"",
            &format!("name='{}'", "x".repeat(MAX_FIELD_BYTES + 1)),
        );
        assert!(parse_report(&fields).is_err());
    }

    #[test]
    fn ignored_content_attribute_detail_and_case_counts_are_bounded() {
        let output = suite(
            &case(&format!(
                "<system-out>{}</system-out>",
                "x".repeat(MAX_ELEMENT_TEXT_BYTES + 1)
            )),
            1,
            0,
            0,
            0,
        );
        assert_eq!(
            parse_report(&output),
            Err(ReportError::LimitExceeded("element text"))
        );
        let properties = format!(
            "<properties>{}</properties>",
            "<property name='x' value='y'/>".repeat(MAX_PROPERTIES + 1)
        );
        assert!(parse_report(&suite(&properties, 0, 0, 0, 0)).is_err());
        let property = format!(
            "<properties><property name='x' value='{}'/></properties>",
            "x".repeat(MAX_ATTRIBUTE_BYTES + 1)
        );
        assert!(parse_report(&suite(&property, 0, 0, 0, 0)).is_err());
        let details = suite(
            &case(&"<failure/>".repeat(MAX_DETAILS_PER_CASE + 1)),
            1,
            1,
            0,
            0,
        );
        assert_eq!(
            parse_report(&details),
            Err(ReportError::LimitExceeded("details per testcase"))
        );
        let cases = "<testcase name='x' time='0'/>".repeat(MAX_CASES + 1);
        assert!(parse_report(&suite(&cases, MAX_CASES, 0, 0, 0)).is_err());
    }

    #[test]
    fn cumulative_budgets_cannot_be_bypassed_by_splitting_nodes() {
        let retained = case(&format!(
            "<failure>{}</failure>",
            "x".repeat(MAX_DETAIL_BYTES)
        ));
        assert_eq!(
            parse_report(&suite(&retained.repeat(17), 17, 17, 0, 0)),
            Err(ReportError::LimitExceeded("retained text"))
        );
        let output = case(&format!(
            "<system-out>{}</system-out>",
            "x".repeat(MAX_ELEMENT_TEXT_BYTES)
        ));
        assert_eq!(
            parse_report(&suite(&output.repeat(9), 9, 0, 0, 0)),
            Err(ReportError::LimitExceeded("total text"))
        );
        let property = format!(
            "<property name='x' value='{}'/>",
            "x".repeat(MAX_ATTRIBUTE_BYTES)
        );
        assert_eq!(
            parse_report(&suite(
                &format!("<properties>{}</properties>", property.repeat(33)),
                0,
                0,
                0,
                0
            )),
            Err(ReportError::LimitExceeded("total attribute size"))
        );
        let events = suite(&"<!---->".repeat(MAX_EVENTS), 0, 0, 0, 0);
        assert_eq!(
            parse_report(&events),
            Err(ReportError::LimitExceeded("XML event count"))
        );
        // The grammar itself rejects deep nesting before the defensive depth cap.
        assert!(parse_report(&suite(&"<properties>".repeat(MAX_DEPTH + 1), 0, 0, 0, 0)).is_err());
    }

    #[test]
    fn validates_optional_timestamps_without_retaining_them() {
        for value in [
            "2024-02-29T23:59:59Z",
            "2026-10-09T06:00:00.125+14:00",
            "2026-10-09T06:00:00",
        ] {
            let xml = suite("", 0, 0, 0, 0).replacen(
                "<testsuite ",
                &format!("<testsuite timestamp='{value}' "),
                1,
            );
            assert!(parse_report(&xml).is_ok(), "{value}");
        }
        for value in [
            "2025-02-29T00:00:00",
            "2026-13-01T00:00:00",
            "2026-01-00T00:00:00",
            "2026-01-01T24:00:00",
            "2026-01-01T00:00:60",
            "2026-01-01T00:00:00+14:01",
            "2026-01-01T00:00:00.",
            "中文中文中文中文中文中文",
        ] {
            let xml = suite("", 0, 0, 0, 0).replacen(
                "<testsuite ",
                &format!("<testsuite timestamp='{value}' "),
                1,
            );
            assert!(parse_report(&xml).is_err(), "{value}");
        }
    }
}
