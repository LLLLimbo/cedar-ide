//! Nonshipping descriptor feasibility evidence, never a recovery implementation.
//! The ordinary tests exercise only memory. The ignored Windows test creates
//! empty default-inherited objects and needs the driver's hard 60-second cap.
//! Neither acceptance nor rejection proves effective access or race resistance.

#[path = "support/windows_privacy_policy.rs"]
mod policy;

#[cfg(windows)]
#[path = "support/windows_privacy_native.rs"]
mod native;

#[cfg(windows)]
#[test]
#[ignore = "explicit CI descriptor feasibility checkpoint; empty generated roots only"]
fn windows_default_inherited_descriptor_probe() {
    let receipt = native::run();
    // Every string in this receipt is a fixed category. Do not add native
    // errors, paths, identities, descriptor bytes, or token details here.
    println!(
        "CEDAR_RECOVERY_DESCRIPTOR_PROBE={}",
        serde_json::to_string(&receipt).expect("fixed probe receipt serialization failed")
    );
    assert!(
        receipt.succeeded(),
        "descriptor probe reported a fixed-category failure"
    );
}
