use application::RegionCatalog;
use domain::ids::{CellCoord, RegionId};
use infrastructure::region::RonRegionCatalog;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
fn main() {
    let out=PathBuf::from(std::env::args_os().nth(1).unwrap());
    let c=RonRegionCatalog::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    for id in ["eastern_interconnection","western_interconnection"] {
        let r=c.load(&RegionId::new(id)).unwrap();
        let t=r.definition.terrain();
        let mut h=Sha256::new();
        h.update(t.width().to_le_bytes()); h.update(t.height().to_le_bytes());
        for y in 0..t.height() { for x in 0..t.width() {
            let v=t.get(CellCoord{x,y}).unwrap(); h.update([u8::from(v.land)]);
            for f in [v.height,v.irradiance,v.wind_potential,v.cooling_water,v.hydro_potential] {h.update(f.value().to_bits().to_le_bytes());}
        }}
        let hex:String=h.finalize().iter().map(|b|format!("{b:02x}")).collect();
        std::fs::write(out.join(format!("{id}-terrain-sha256.txt")),format!("{hex}\n")).unwrap();
        println!("{id}: {hex}");
    }
}
