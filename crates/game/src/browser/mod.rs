//! Browser startup: fetch and verify one complete deployment before opening the game.
use crate::{
    GamePlugins,
    l10n::Localizer,
    sim::{Catalog, SimSpeed},
    state::AppState,
};
use bevy::prelude::*;
use infrastructure::{region::BundleRegionCatalog, web_assets::AssetManifest};
mod probe;
use std::collections::BTreeMap;
use wasm_bindgen::{JsCast, JsValue};

#[derive(Resource)]
pub struct WebAssets(pub BTreeMap<String, Vec<u8>>);
const BUILD: &str = match option_env!("MM_BUILD_SHA") {
    Some(v) => v,
    None => "development",
};

async fn fetch(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let response: web_sys::Response =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "asset HTTP {}",
            response.status()
        )));
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
async fn assets() -> Result<WebAssets, JsValue> {
    let inventory = fetch("assets/manifest.ron").await?;
    let manifest: AssetManifest =
        ron::de::from_bytes(&inventory).map_err(|e| JsValue::from_str(&e.to_string()))?;
    if manifest.version != 1 || manifest.build != BUILD {
        return Err(JsValue::from_str("deployment changed"));
    }
    let mut files = BTreeMap::new();
    for file in manifest.assets {
        if file.url.contains('/') || file.url.contains("..") || file.length > 8 * 1024 * 1024 {
            return Err(JsValue::from_str("invalid asset inventory"));
        }
        let bytes = fetch(&format!("assets/{}", file.url)).await?;
        if !file.verifies(&bytes) {
            return Err(JsValue::from_str("asset checksum differs"));
        }
        if files.insert(file.name, bytes).is_some() {
            return Err(JsValue::from_str("duplicate asset"));
        }
    }
    Ok(WebAssets(files))
}
pub fn run() {
    wasm_bindgen_futures::spawn_local(async {
        match assets().await.and_then(launch) {
            Ok(()) => (),
            Err(error) => {
                web_sys::console::error_1(&error);
                if let Some(status) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("status"))
                {
                    status.set_attribute("data-failed", "true").ok();
                    status
                        .set_text_content(Some("A new version may be available. Reload to retry."));
                }
            }
        }
    });
}
fn launch(assets: WebAssets) -> Result<(), JsValue> {
    let sources = assets
        .0
        .iter()
        .filter(|(name, _)| name.starts_with("locales/en-US/"))
        .map(|(name, bytes)| String::from_utf8(bytes.clone()).map(|source| (name.clone(), source)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let localizer =
        Localizer::from_sources(sources).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let bundles = assets
        .0
        .iter()
        .filter_map(|(name, bytes)| {
            name.strip_prefix("regions/")
                .and_then(|s| s.strip_suffix(".bundle"))
                .map(|id| (domain::ids::RegionId::new(id), bytes.clone()))
        })
        .collect();
    let mut settings = crate::preferences::ClientSettings::default();
    settings.loaded = true;
    if let Some(link) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("source"))
    {
        link.set_attribute(
            "href",
            &format!("https://github.com/adsouza/megawatt-mogul/tree/{BUILD}"),
        )
        .ok();
    }
    let mut app = App::new();
    app.insert_resource(assets)
        .insert_resource(localizer)
        .insert_resource(Catalog(BundleRegionCatalog::new(bundles)))
        .insert_resource(settings)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Megawatt Mogul".into(),
                canvas: Some("#game".into()),
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GamePlugins)
        .add_systems(PreUpdate, visibility)
        .add_systems(Last, readiness);
    app.run();
    Ok(())
}
fn visibility(mut speed: ResMut<SimSpeed>) {
    if web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden())
    {
        *speed = SimSpeed::Paused;
    }
}
fn readiness(state: Res<State<AppState>>, localizer: Res<Localizer>) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Some(body) = document.body() {
        body.set_attribute("data-app-state", &format!("{:?}", state.get()))
            .ok();
    }
    if *state.get() != AppState::Boot {
        if let Some(status) = document.get_element_by_id("status") {
            status.set_attribute("hidden", "").ok();
        }
        if let Some(body) = document.body() {
            body.set_attribute("data-ready", "true").ok();
        }
    }
    if let Some(banner) = document.get_element_by_id("preview") {
        banner.set_text_content(Some(&localizer.text("browser-preview")));
    }
}
