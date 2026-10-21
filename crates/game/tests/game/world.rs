//! Plan 3's CPU geometry contract: no window, renderer or simulation mutation.
use bevy::math::Vec3;
use domain::{
    ids::CellCoord,
    region::{Cell, TerrainGrid},
    units::Ratio,
};
use game::world::{GridTransform, TerrainGeometry, surface_height};
use proptest::prelude::*;

fn grid(width: u16, height: u16, heights: &[f64]) -> TerrainGrid {
    let cells = heights
        .iter()
        .map(|height| Cell {
            land: *height >= 0.0,
            height: Ratio::new(height.max(0.0)).unwrap(),
            irradiance: Ratio::ZERO,
            cooling_water: Ratio::ZERO,
            hydro_potential: Ratio::ZERO,
        })
        .collect();
    TerrainGrid::new(width, height, cells).unwrap()
}
fn valid(mesh: &TerrainGeometry) {
    assert_eq!(mesh.positions.len(), mesh.normals.len());
    assert_eq!(mesh.positions.len(), mesh.colors.len());
    assert_eq!(mesh.indices.len() % 3, 0);
    for (positions, normals) in mesh.positions.iter().zip(&mesh.normals) {
        assert!(positions.iter().all(|v| v.is_finite()));
        assert!((Vec3::from_array(*normals).length() - 1.0).abs() < 1e-6);
    }
    for face in mesh.indices.as_chunks::<3>().0 {
        let p: Vec<Vec3> = face
            .iter()
            .map(|index| Vec3::from_array(mesh.positions[*index as usize]))
            .collect();
        let normal = (p[1] - p[0]).cross(p[2] - p[0]);
        // Adjacent shipped cells can differ by a single f32 ULP: a tiny vertical skirt is
        // still a real face. Reject zero area rather than imposing an arbitrary visual scale.
        assert!(normal.is_finite() && normal.length_squared() > 0.0, "{p:?}");
        assert!(
            normal
                .normalize()
                .dot(Vec3::from_array(mesh.normals[face[0] as usize]))
                > 0.999
        );
    }
}

#[test]
fn world_flat_terrain_has_complete_tops_a_closed_base_and_consistent_normals() {
    let terrain = grid(2, 2, &[0.0, 0.0, 0.0, 0.0]);
    let mesh = TerrainGeometry::build(&terrain, &[[64, 128, 70]; 4]).unwrap();
    valid(&mesh);
    // Four cell tops, eight outer side quads, one bottom. No internal coincident walls.
    assert_eq!(mesh.indices.len(), 13 * 6);
    let grid = GridTransform::new(2, 2);
    assert_eq!(grid.cell_at(-1.0, -1.0), Some(CellCoord { x: 0, y: 0 }));
    assert_eq!(grid.cell_at(1.0, 0.0), None);
    assert_eq!(grid.cell_at(0.0, 1.0), None);
    assert_eq!(grid.cell_at(f32::NAN, 0.0), None);
}
#[test]
fn world_height_and_water_are_shared_by_meshes_and_object_placement() {
    let terrain = grid(2, 1, &[-1.0, 1.0]);
    let mesh = TerrainGeometry::build(&terrain, &[[28, 52, 96], [150, 130, 100]]).unwrap();
    valid(&mesh);
    let transform = GridTransform::new(2, 1);
    for x in 0..2 {
        let cell = CellCoord { x, y: 0 };
        let center = transform.cell_center(&terrain, cell).unwrap();
        assert_eq!(transform.cell_at(center.x, center.z), Some(cell));
        assert_eq!(center.y, surface_height(&terrain, cell).unwrap());
        assert!(mesh.positions.iter().any(|p| p[1] == center.y));
    }
    assert_eq!(
        surface_height(&terrain, CellCoord { x: 0, y: 0 }),
        Some(0.0)
    );
    assert!(surface_height(&terrain, CellCoord { x: 1, y: 0 }).unwrap() > 1.0);
    assert!(TerrainGeometry::build(&terrain, &[]).is_err());
    assert!(GridTransform::new(0, 1).cell_at(0.0, 0.0).is_none());
    let empty = TerrainGrid::new(0, 0, vec![]).unwrap();
    assert!(
        TerrainGeometry::build(&empty, &[])
            .unwrap()
            .positions
            .is_empty()
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn world_geometry_is_finite_complete_and_deterministic(width in 1u16..9,height in 1u16..9, values in prop::collection::vec(0u16..1001,64)) {
        let elevations:Vec<_> = values[..usize::from(width)*usize::from(height)].iter().map(|v|f64::from(*v)/1000.0).collect();
        let terrain = grid(width,height,&elevations);
        let colors = vec![[70,170,90];elevations.len()];
        let mesh = TerrainGeometry::build(&terrain,&colors).unwrap(); valid(&mesh);
        prop_assert_eq!(&mesh,&TerrainGeometry::build(&terrain,&colors).unwrap());
        let coordinates = GridTransform::new(width,height);
        for y in 0..height { for x in 0..width {
            let cell = CellCoord{x,y}; let at = coordinates.cell_center(&terrain,cell).unwrap();
            prop_assert_eq!(coordinates.cell_at(at.x,at.z),Some(cell));
            let tops = mesh.normals.iter().filter(|n|**n==[0.0,1.0,0.0]).count();
            prop_assert_eq!(tops,usize::from(width)*usize::from(height)*4);
        }}
    }
}

#[test]
fn world_both_shipped_regions_build_with_existing_resource_layers_without_changing_geometry() {
    use application::RegionCatalog;
    use domain::ids::RegionId;
    let catalog = infrastructure::region::RonRegionCatalog::new(
        crate::support::shipped_assets().join("regions"),
    );
    for id in ["eastern_interconnection", "western_interconnection"] {
        let region = catalog.load(&RegionId::new(id)).unwrap();
        let terrain =
            game::world::region_terrain(&region.definition, game::hud::MapLayer::Terrain).unwrap();
        valid(&terrain);
        let irradiance =
            game::world::region_terrain(&region.definition, game::hud::MapLayer::Irradiance)
                .unwrap();
        assert_eq!(terrain.positions, irradiance.positions);
        assert_eq!(terrain.normals, irradiance.normals);
        assert_eq!(terrain.indices, irradiance.indices);
        assert_ne!(terrain.colors, irradiance.colors);
    }
}
