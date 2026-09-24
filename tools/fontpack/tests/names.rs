//! §7 file names.
use pulp_fontpack::names::{license_file_name, manifest_file_name, pack_file_name};

#[test]
fn spec_examples() {
    // §7: Iansui at 24 px is IANSUI24.PFP, sidecar IANSUOFL.TXT
    assert_eq!(pack_file_name("IANSUI", 24).unwrap(), "IANSUI24.PFP");
    assert_eq!(license_file_name("IANSUI").unwrap(), "IANSUOFL.TXT");
    assert_eq!(pack_file_name("A1", 8).unwrap(), "A108.PFP");
    assert_eq!(license_file_name("AB").unwrap(), "ABOFL.TXT");
    assert_eq!(manifest_file_name("IANSUI", 24).unwrap(), "IANSUI24.TXT");
}

#[test]
fn rejects_names_that_are_not_8_3() {
    for fam in ["", "IANSUIX", "iansui", "IAN-UI", "IANSÜ"] {
        assert!(pack_file_name(fam, 24).is_err(), "{fam:?}");
        assert!(license_file_name(fam).is_err(), "{fam:?}");
    }
    assert!(pack_file_name("IANSUI", 7).is_err());
    assert!(pack_file_name("IANSUI", 97).is_err());
}
