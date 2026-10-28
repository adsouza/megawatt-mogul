use domain::{
    region::{PlantKind, SitingRule},
    state::{CURRENT_STATE_SCHEMA, State},
    units::Ratio,
};
#[test]
fn geothermal_wire_contract_is_schema6_and_kind8() {
    assert_eq!(CURRENT_STATE_SCHEMA, 6);
    let kind = PlantKind::from_state_bytes_schema(&[8], 6).unwrap();
    assert!(kind.is_dispatchable());
    assert!(!kind.is_variable());
    assert_eq!(kind.state_bytes(), vec![8]);
    for schema in 1..=5 {
        assert!(PlantKind::from_state_bytes_schema(&[8], schema).is_err());
    }
    let mut site = vec![3];
    site.extend(Ratio::ONE.state_bytes());
    assert!(SitingRule::from_state_bytes_schema(&site, 6).is_ok());
    for schema in 1..=5 {
        assert!(SitingRule::from_state_bytes_schema(&site, schema).is_err());
    }
}
