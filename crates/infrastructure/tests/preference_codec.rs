use infrastructure::client_preferences::{
    ClientPreferences, decode_preferences, encode_preferences,
};
#[test]
fn byte_preferences_preserve_validation_and_do_not_require_a_directory() {
    let mut settings = ClientPreferences {
        zoom: 1.3,
        effects: false,
        ..Default::default()
    };
    let bytes = encode_preferences(&settings).unwrap();
    assert_eq!(decode_preferences(&bytes).unwrap(), settings);
    settings.zoom = f32::NAN;
    assert!(encode_preferences(&settings).is_err());
    assert!(decode_preferences(b"(version:999)").is_err());
    assert!(decode_preferences(&vec![b' '; 128 * 1024 + 1]).is_err());
}

proptest::proptest! {
    #[test]
    fn valid_preferences_round_trip_exactly(zoom in 0.2_f32..5.0, effects in proptest::bool::ANY,
        width in 1.0_f32..4000.0, height in 1.0_f32..2000.0) {
        let mut settings=ClientPreferences {zoom,effects,..Default::default()};
        settings.panels.insert("planning".into(),[width,height]);
        proptest::prop_assert_eq!(decode_preferences(&encode_preferences(&settings).unwrap()).unwrap(),settings);
    }
}
