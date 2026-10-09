mod common;
use common::*;

#[test]
fn explicit_license_metadata_does_not_claim_ofl() {
    let tmp = TempDir::new();
    let license = tmp.write(
        "license.txt",
        b"Permission granted for this test fixture.\n",
    );
    let out = tmp.join("out");
    let run = Args::base(&bookerly_path(), &license, &out)
        .with("--sizes", "16")
        .with("--license-name", "Test permission")
        .run();
    assert_eq!(run.code, 0, "{}", run.stderr);
    let prov = kv(&std::fs::read(out.join("PROV.TXT")).unwrap());
    assert_eq!(get(&prov, "license_name"), "Test permission");
    assert_eq!(get(&prov, "license_file"), "LICENSE.TXT");
    assert_eq!(
        std::fs::read(out.join("LICENSE.TXT")).unwrap(),
        std::fs::read(license).unwrap()
    );
    assert!(!out.join("OFL.TXT").exists());
}

#[test]
fn license_metadata_cannot_inject_provenance_lines() {
    let tmp = TempDir::new();
    let license = tmp.write("license.txt", b"test\n");
    for name in ["", " Test", "Test\nlicense_name=SIL OFL 1.1", "測試"] {
        let out = tmp.join("out");
        let run = Args::base(&bookerly_path(), &license, &out)
            .with("--license-name", name)
            .run();
        assert_clean_failure(&run, 2, name);
        assert!(!out.exists());
    }
}
