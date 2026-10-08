//! Explicit, read-only disk/index Git views. Execution trust is still required:
//! repository clean/process filters may run with the user's full authority.
use crate::{error, io_error, BackendMode, RawCommandResult, Workspace, MAX_COMMAND_OUTPUT_BYTES};
use cedar_protocol::{GitChange, GitChangeKind, GitDiffKind, Operation, Payload, RemoteError};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ENTRIES: usize = 4096;
const MAX_PATH: usize = 4096;
const MAX_METADATA_ENTRIES: usize = 32_768;
#[cfg(windows)]
const NULL_FILE: &str = "NUL";
#[cfg(not(windows))]
const NULL_FILE: &str = "/dev/null";

pub(super) fn platform_supported(mode: BackendMode) -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
        || (cfg!(windows) && mode == BackendMode::IsolatedAgent)
}

impl Workspace {
    pub(super) fn handle_git_read(&self, operation: Operation) -> Result<Payload, RemoteError> {
        // Must precede executable, repository, environment, and path inspection.
        if !self.allow_run {
            return Err(error("run_disabled", "Git can execute repository-configured clean/process filters; enable workspace execution trust first"));
        }
        if !platform_supported(self.backend_mode) {
            return Err(error(
                "unsupported_platform",
                "Git changes and diff require Linux/macOS, or an isolated Windows agent",
            ));
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let executable = match &operation {
            Operation::GitChanges { git_executable }
            | Operation::GitDiff { git_executable, .. } => git_executable,
            _ => unreachable!("Git read handler only accepts its two operations"),
        };
        let recipe = Recipe::new(&self.root, executable, deadline)?;
        recipe.require_version()?;
        recipe.require_nonbare()?;
        let entries = recipe.changes()?;
        match operation {
            Operation::GitChanges { .. } => Ok(Payload::GitChanges { entries }),
            Operation::GitDiff { path, kind, .. } => {
                // The status above is reacquired on every diff request, under
                // the same total deadline as the version check and patch.
                validate_file_path(&recipe.root, &path)?;
                let eligible = entries.iter().any(|entry| {
                    entry.path == path
                        && match kind {
                            GitDiffKind::Staged => entry.can_diff_staged,
                            GitDiffKind::Unstaged => entry.can_diff_unstaged,
                        }
                });
                if !eligible {
                    return Err(error("git_diff_unavailable", "The selected path no longer has an eligible change of this kind; refresh Git changes"));
                }
                let text = recipe.diff(&path, kind)?;
                Ok(Payload::GitDiff { path, kind, text })
            }
            _ => unreachable!(),
        }
    }
}

struct Recipe {
    executable: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    environment: Vec<(OsString, OsString)>,
    deadline: Instant,
}

impl Recipe {
    fn new(root: &Path, executable: &str, deadline: Instant) -> Result<Self, RemoteError> {
        let executable = validate_executable(executable)?;
        let root = ordinary_path(root)?;
        let git_dir = root.join(".git");
        validate_repository(&root, &git_dir, deadline)?;
        Ok(Self {
            executable,
            root,
            git_dir,
            environment: child_environment(std::env::vars_os())?,
            deadline,
        })
    }

    fn arguments(&self, command: &[&str]) -> Result<Vec<String>, RemoteError> {
        let path_text = |path: &Path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| error("invalid_path", "Git repository paths must be valid UTF-8"))
        };
        let mut args: Vec<String> = [
            "--no-pager",
            "--no-optional-locks",
            "--no-lazy-fetch",
            "--no-replace-objects",
            "--literal-pathspecs",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        args.push(format!("--git-dir={}", path_text(&self.git_dir)?));
        args.push(format!("--work-tree={}", path_text(&self.root)?));
        for setting in [
            "core.fsmonitor=false".to_owned(),
            "core.untrackedCache=false".to_owned(),
            "diff.autoRefreshIndex=false".to_owned(),
            format!("core.attributesFile={NULL_FILE}"),
            format!("diff.orderFile={NULL_FILE}"),
            format!("core.hooksPath={NULL_FILE}"),
        ] {
            args.extend(["-c".to_owned(), setting]);
        }
        args.extend(command.iter().map(|value| (*value).to_owned()));
        Ok(args)
    }

    fn run(&self, command: &[&str]) -> Result<RawCommandResult, RemoteError> {
        check_deadline(self.deadline)?;
        let args = self.arguments(command)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let result = {
            let mut process = std::process::Command::new(&self.executable);
            process
                .args(args)
                .current_dir(&self.root)
                .env_clear()
                .envs(self.environment.iter().map(|(name, value)| (name, value)));
            crate::run_bounded_raw(
                process,
                self.deadline.saturating_duration_since(Instant::now()),
            )?
        };
        #[cfg(windows)]
        let result = windows::run(self, args)?;
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let result: RawCommandResult = return Err(error(
            "unsupported_platform",
            "Git process ownership is unavailable on this platform",
        ));
        // Cleanup joins are always performed by the owner, even at the deadline.
        if result.timed_out || Instant::now() >= self.deadline {
            return Err(error(
                "command_timeout",
                "Git request exceeded its ten-second deadline",
            ));
        }
        if result.truncated {
            return Err(error(
                "output_limit",
                "Git output exceeded 256 KiB per stream or could not be captured completely",
            ));
        }
        Ok(result)
    }

    fn require_version(&self) -> Result<(), RemoteError> {
        // Actually pass the option to Git; an ignored environment variable does
        // not establish the no-lazy-fetch guarantee on older installations.
        let result = self.run(&["--version"])?;
        if result.exit_code != Some(0) || !supported_version(&result.stdout) {
            return Err(error("unsupported_git_version", "Git 2.45 or newer with --no-lazy-fetch support is required; select a newer explicit Git executable"));
        }
        Ok(())
    }

    fn require_nonbare(&self) -> Result<(), RemoteError> {
        let result = self.run(&["config", "--local", "--type=bool", "--get", "core.bare"])?;
        if result.exit_code == Some(0)
            && matches!(result.stdout.as_slice(), b"false\n" | b"false\r\n")
        {
            return Ok(());
        }
        // Absence is the ordinary default; every other response fails closed.
        if result.exit_code == Some(1) && result.stdout.is_empty() && result.stderr.is_empty() {
            return Ok(());
        }
        Err(error(
            "git_repository_unsupported",
            "Only an ordinary non-bare repository rooted at the workspace is supported",
        ))
    }

    fn changes(&self) -> Result<Vec<GitChange>, RemoteError> {
        let result = self.run(&[
            "status",
            "--porcelain=v2",
            "-z",
            "--no-renames",
            "--ignore-submodules=all",
            "--untracked-files=all",
            "--",
            ".",
        ])?;
        let bytes = success(result)?;
        let mut entries = parse_status(&bytes)?;
        for entry in &mut entries {
            check_deadline(self.deadline)?;
            if validate_file_path(&self.root, &entry.path).is_err() {
                entry.kind = GitChangeKind::Unsupported;
                entry.can_diff_staged = false;
                entry.can_diff_unstaged = false;
            }
        }
        check_deadline(self.deadline)?;
        Ok(entries)
    }

    fn diff(&self, path: &str, kind: GitDiffKind) -> Result<String, RemoteError> {
        // Recheck the metadata boundary and the selected file immediately before
        // execution. This is not a filesystem sandbox against concurrent writers.
        validate_repository(&self.root, &self.git_dir, self.deadline)?;
        validate_file_path(&self.root, path)?;
        let mut args = vec![
            "diff",
            "--patch",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--ignore-submodules=all",
            "--no-color",
            "--no-relative",
            "--unified=3",
            "--inter-hunk-context=0",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ];
        if kind == GitDiffKind::Staged {
            // No HEAD argument: Git handles an unborn branch itself.
            args.push("--cached");
        }
        args.extend(["--", path]);
        let bytes = success(self.run(&args)?)?;
        String::from_utf8(bytes).map_err(|_| error("invalid_utf8", "Git diff is not valid UTF-8"))
    }
}

fn success(result: RawCommandResult) -> Result<Vec<u8>, RemoteError> {
    if result.exit_code != Some(0) {
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        return Err(error(
            "git_error",
            format!("Git failed: {}", diagnostic.trim()),
        ));
    }
    Ok(result.stdout)
}

fn supported_version(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Some(version) = text.trim_end().strip_prefix("git version ") else {
        return false;
    };
    let Some(version) = version.split_whitespace().next() else {
        return false;
    };
    let mut fields = version.split('.');
    let Some(major) = fields.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    let Some(minor) = fields.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    let Some(patch) = fields.next() else {
        return false;
    };
    !patch.is_empty()
        && patch.bytes().all(|byte| byte.is_ascii_digit())
        && (major > 2 || (major == 2 && minor >= 45))
}

fn check_deadline(deadline: Instant) -> Result<(), RemoteError> {
    if Instant::now() >= deadline {
        Err(error(
            "command_timeout",
            "Git request exceeded its ten-second deadline",
        ))
    } else {
        Ok(())
    }
}

fn child_environment(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Result<Vec<(OsString, OsString)>, RemoteError> {
    let mut environment = Vec::new();
    for (name, value) in inherited {
        if name.as_encoded_bytes().first() == Some(&b'=')
            || environment_key_matches(&name, "GIT_", true)?
            || environment_key_matches(&name, "LC_ALL", false)?
            || environment_key_matches(&name, "LANG", false)?
            || environment_key_matches(&name, "LANGUAGE", false)?
        {
            continue;
        }
        // Retain original OS strings, including unrelated non-Unicode data.
        environment.push((name, value));
    }
    environment.extend(
        [
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_SYSTEM", NULL_FILE),
            ("GIT_CONFIG_GLOBAL", NULL_FILE),
            ("GIT_CONFIG_COUNT", "0"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("GIT_NO_REPLACE_OBJECTS", "1"),
            ("GIT_LITERAL_PATHSPECS", "1"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_ALLOW_PROTOCOL", ""),
            ("GIT_ATTR_NOSYSTEM", "1"),
            ("LC_ALL", "C"),
            ("LANG", "C"),
            ("LANGUAGE", "C"),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value.into())),
    );
    Ok(environment)
}

fn environment_key_matches(name: &OsStr, target: &str, prefix: bool) -> Result<bool, RemoteError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Globalization::{
            CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
        };
        // Windows Git uses GetEnvironmentVariableW, whose ordinal OS case
        // comparison includes aliases such as dotless-i. ASCII/Unicode Rust
        // lowercasing does not establish the same boundary. Bound the prefix
        // inspection without lossy conversion, preserving all retained pairs.
        let target: Vec<u16> = target.encode_utf16().collect();
        let name: Vec<u16> = name
            .encode_wide()
            .take(target.len() + usize::from(!prefix))
            .collect();
        if name.len() != target.len() {
            return Ok(false);
        }
        // SAFETY: live nonempty short slices have explicit i32 lengths; TRUE
        // is exactly 1. No terminator or Unicode normalization is required.
        let comparison = unsafe {
            CompareStringOrdinal(
                name.as_ptr(),
                name.len() as i32,
                target.as_ptr(),
                target.len() as i32,
                1,
            )
        };
        match comparison {
            CSTR_EQUAL => Ok(true),
            CSTR_LESS_THAN | CSTR_GREATER_THAN => Ok(false),
            _ => Err(error(
                "invalid_command",
                "Windows environment name comparison failed",
            )),
        }
    }
    #[cfg(not(windows))]
    {
        let bytes = name.as_encoded_bytes();
        Ok(if prefix {
            bytes
                .get(..target.len())
                .is_some_and(|bytes| bytes.eq_ignore_ascii_case(target.as_bytes()))
        } else {
            bytes.eq_ignore_ascii_case(target.as_bytes())
        })
    }
}

fn validate_executable(text: &str) -> Result<PathBuf, RemoteError> {
    let path = Path::new(text);
    if text.is_empty() || text.len() > MAX_PATH || text.contains('\0') || !path.is_absolute() {
        return Err(error(
            "invalid_command",
            "Git executable must be an explicit existing absolute path",
        ));
    }
    #[cfg(windows)]
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(error(
            "invalid_command",
            "Windows Git executable must be a native .exe",
        ));
    }
    // Explicit symlinked executables are resolved by the OS, as on the existing
    // command route. Repository metadata and selected files have stricter rules.
    if !fs::metadata(path).map_err(io_error)?.is_file() {
        return Err(error(
            "invalid_command",
            "Git executable must be a regular file",
        ));
    }
    ordinary_path(path)
}

fn ordinary_path(path: &Path) -> Result<PathBuf, RemoteError> {
    #[cfg(windows)]
    {
        // Workspace::open canonicalizes to a verbatim path. Use only the exact
        // corresponding local-drive spelling accepted by the Windows owner.
        use std::path::Prefix;
        let text = path
            .to_str()
            .ok_or_else(|| error("invalid_path", "Git paths must be UTF-8"))?;
        let ordinary = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => PathBuf::from(
                    text.strip_prefix(r"\\?\")
                        .ok_or_else(|| error("invalid_path", "Invalid Git drive path"))?,
                ),
                Prefix::Disk(_) if path.is_absolute() => path.to_owned(),
                _ => return Err(error("invalid_path", "Git requires a local-drive path")),
            },
            _ => {
                return Err(error(
                    "invalid_path",
                    "Git requires an absolute local-drive path",
                ))
            }
        };
        if ordinary.canonicalize().map_err(io_error)? != path.canonicalize().map_err(io_error)? {
            return Err(error(
                "invalid_path",
                "Git path spelling changed filesystem identity",
            ));
        }
        Ok(ordinary)
    }
    #[cfg(not(windows))]
    Ok(path.to_owned())
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    meta.file_type().is_symlink()
}

fn unsupported_repository() -> RemoteError {
    error("git_repository_unsupported", "Only ordinary workspace-root repositories with a real .git directory are supported; linked/shared repositories, alternates, and symlink/reparse metadata are unsupported")
}

fn validate_repository(root: &Path, git_dir: &Path, deadline: Instant) -> Result<(), RemoteError> {
    let root_meta = fs::symlink_metadata(root).map_err(io_error)?;
    if is_link(&root_meta) || !root_meta.is_dir() {
        return Err(unsupported_repository());
    }
    let meta = fs::symlink_metadata(git_dir).map_err(|_| unsupported_repository())?;
    if is_link(&meta) || !meta.is_dir() {
        return Err(unsupported_repository());
    }
    for rejected in [
        "commondir",
        "gitdir",
        "objects/info/alternates",
        "objects/info/http-alternates",
    ] {
        match fs::symlink_metadata(git_dir.join(rejected)) {
            Ok(_) => return Err(unsupported_repository()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
    }
    // Walk without following links. This covers refs, object fanout/pack files,
    // index/sharedindex, config and other metadata, with explicit work bounds.
    let mut pending = vec![git_dir.to_owned()];
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        check_deadline(deadline)?;
        for child in fs::read_dir(directory).map_err(io_error)? {
            check_deadline(deadline)?;
            visited += 1;
            if visited > MAX_METADATA_ENTRIES {
                return Err(error(
                    "git_repository_unsupported",
                    "Git metadata exceeds the 32768-entry inspection limit",
                ));
            }
            let child = child.map_err(io_error)?;
            let meta = fs::symlink_metadata(child.path()).map_err(io_error)?;
            if is_link(&meta) || (!meta.is_file() && !meta.is_dir()) {
                return Err(unsupported_repository());
            }
            if meta.is_dir() {
                pending.push(child.path());
            }
        }
    }
    for (relative, directory) in [("HEAD", false), ("objects", true), ("refs", true)] {
        let meta =
            fs::symlink_metadata(git_dir.join(relative)).map_err(|_| unsupported_repository())?;
        if is_link(&meta)
            || if directory {
                !meta.is_dir()
            } else {
                !meta.is_file()
            }
        {
            return Err(unsupported_repository());
        }
    }
    Ok(())
}

fn validate_file_path(root: &Path, text: &str) -> Result<(), RemoteError> {
    let relative = crate::validate_path(text)?;
    if text.is_empty()
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part.eq_ignore_ascii_case(".git"))
    {
        return Err(error(
            "invalid_path",
            "Git diff requires an exact relative file path outside .git",
        ));
    }
    let canonical_root = root.canonicalize().map_err(io_error)?;
    let metadata_boundary = match root.join(".git").canonicalize() {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => canonical_root.join(".git"),
        Err(e) => return Err(io_error(e)),
    };
    let mut current = root.to_owned();
    let components: Vec<_> = relative.components().collect();
    let mut missing = false;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(error(
                "invalid_path",
                "Git path must contain only ordinary relative components",
            ));
        };
        #[cfg(windows)]
        {
            let name = name
                .to_str()
                .ok_or_else(|| error("invalid_path", "Git path must be UTF-8"))?;
            let stem = name
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_end_matches(' ');
            let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
                .iter()
                .any(|device| stem.eq_ignore_ascii_case(device))
                || (stem.as_bytes().get(..3).is_some_and(|prefix| {
                    prefix.eq_ignore_ascii_case(b"COM") || prefix.eq_ignore_ascii_case(b"LPT")
                }) && matches!(
                    stem.get(3..),
                    Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                ));
            if name.ends_with(['.', ' '])
                || name.chars().any(char::is_control)
                || name.contains(['<', '>', '"', '|', '?', '*'])
                || device
            {
                return Err(error(
                    "invalid_path",
                    "Git path has an ambiguous Windows file name",
                ));
            }
        }
        current.push(name);
        if missing {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if is_link(&meta)
                    || if index + 1 == components.len() {
                        !meta.is_file()
                    } else {
                        !meta.is_dir()
                    }
                {
                    return Err(error(
                        "invalid_path",
                        "Git diff paths must be regular files without symlink/reparse ancestors",
                    ));
                }
                let canonical = current.canonicalize().map_err(io_error)?;
                if !canonical.starts_with(&canonical_root)
                    || canonical.starts_with(&metadata_boundary)
                {
                    return Err(error(
                        "invalid_path",
                        "Git file identity crosses the workspace or .git metadata boundary",
                    ));
                }
            }
            // A deleted directory can leave several absent suffix components.
            // Lexical checks above still cover every component of that suffix.
            Err(e) if e.kind() == io::ErrorKind::NotFound => missing = true,
            Err(e) => return Err(io_error(e)),
        }
    }
    Ok(())
}

fn malformed() -> RemoteError {
    error(
        "git_status_invalid",
        "Git returned malformed or unsupported porcelain-v2 status",
    )
}

fn parse_status(bytes: &[u8]) -> Result<Vec<GitChange>, RemoteError> {
    if bytes.len() > MAX_COMMAND_OUTPUT_BYTES {
        return Err(error(
            "output_limit",
            "Git status exceeded its output bound",
        ));
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) {
        return Err(malformed());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| error("invalid_utf8", "Git status contains a non-UTF-8 path"))?;
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    for record in text[..text.len() - 1].split('\0') {
        if entries.len() == MAX_ENTRIES {
            return Err(error("output_limit", "Git status exceeded 4096 entries"));
        }
        let mut entry = if let Some(path) = record.strip_prefix("? ") {
            GitChange {
                path: path.into(),
                index: '?',
                worktree: '?',
                kind: GitChangeKind::Untracked,
                can_diff_staged: false,
                can_diff_unstaged: false,
            }
        } else if record.starts_with("1 ") {
            let parts: Vec<_> = record.splitn(9, ' ').collect();
            if parts.len() != 9
                || !valid_xy(parts[1], false)
                || !valid_sub(parts[2])
                || !parts[3..6].iter().all(|mode| valid_mode(mode))
                || !valid_hashes(&parts[6..8])
            {
                return Err(malformed());
            }
            let regular = parts[2] == "N..."
                && parts[3..6]
                    .iter()
                    .all(|mode| matches!(*mode, "000000" | "100644" | "100755"));
            let xy = parts[1].as_bytes();
            GitChange {
                path: parts[8].into(),
                index: xy[0] as char,
                worktree: xy[1] as char,
                kind: if regular {
                    GitChangeKind::File
                } else {
                    GitChangeKind::Unsupported
                },
                can_diff_staged: regular && xy[0] != b'.',
                can_diff_unstaged: regular && xy[1] != b'.',
            }
        } else if record.starts_with("u ") {
            let parts: Vec<_> = record.splitn(11, ' ').collect();
            if parts.len() != 11
                || !valid_xy(parts[1], true)
                || !valid_sub(parts[2])
                || !parts[3..7].iter().all(|mode| valid_mode(mode))
                || !valid_hashes(&parts[7..10])
            {
                return Err(malformed());
            }
            let xy = parts[1].as_bytes();
            GitChange {
                path: parts[10].into(),
                index: xy[0] as char,
                worktree: xy[1] as char,
                kind: GitChangeKind::Conflict,
                can_diff_staged: false,
                can_diff_unstaged: false,
            }
        } else {
            return Err(malformed());
        };
        if entry.path.is_empty() || entry.path.len() > MAX_PATH || !paths.insert(entry.path.clone())
        {
            return Err(malformed());
        }
        // Unknown/unrepresentable lexical paths are status-only. The exact UTF-8
        // identity is retained for an escaped UI label; it is never normalized.
        if crate::validate_path(&entry.path).is_err() {
            entry.kind = GitChangeKind::Unsupported;
            entry.can_diff_staged = false;
            entry.can_diff_unstaged = false;
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn valid_xy(xy: &str, conflict: bool) -> bool {
    if conflict {
        matches!(xy, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
    } else {
        xy.len() == 2
            && xy != ".."
            && xy
                .bytes()
                .all(|byte| matches!(byte, b'.' | b'M' | b'A' | b'D' | b'T'))
    }
}
fn valid_sub(sub: &str) -> bool {
    let bytes = sub.as_bytes();
    sub == "N..."
        || (bytes.len() == 4
            && bytes[0] == b'S'
            && matches!(bytes[1], b'.' | b'C')
            && matches!(bytes[2], b'.' | b'M')
            && matches!(bytes[3], b'.' | b'U'))
}
fn valid_mode(mode: &str) -> bool {
    matches!(mode, "000000" | "100644" | "100755" | "120000" | "160000")
}
fn valid_hashes(hashes: &[&str]) -> bool {
    hashes.iter().all(|hash| {
        matches!(hash.len(), 40 | 64)
            && hash.len() == hashes[0].len()
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;
