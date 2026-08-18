use ailloli_ui_core::FontId;
use fontique::{
    Attributes, Blob, Collection, CollectionOptions, FallbackKey, GenericFamily, QueryFamily,
    QueryFont, QueryStatus, Script, SourceCache, SourceCacheOptions,
};

const JBM_NERD_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Font database via `fontique`: bundled assets + system fonts for Parley layout.
#[derive(Clone)]
pub struct FontDb {
    collection: Collection,
    source_cache: SourceCache,
    mono_family: Option<fontique::FamilyId>,
}

impl Default for FontDb {
    fn default() -> Self {
        Self::new()
    }
}

impl FontDb {
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        let source_cache = SourceCache::new(SourceCacheOptions::default());

        // 1) Register at least one known font from bundled assets.
        let mono_family = {
            let blob: Blob<u8> = Blob::from(JBM_NERD_REGULAR.to_vec());
            let families = collection.register_fonts(blob, None);
            families.first().map(|(id, _)| *id)
        };

        // 2) Optionally load from `assets/fonts` (invalid paths are ignored).
        collection.load_fonts_from_paths(["assets/fonts"]);

        Self {
            collection,
            source_cache,
            mono_family,
        }
    }

    pub fn families_for(&self, font_id: FontId) -> Vec<QueryFamily<'static>> {
        match font_id {
            FontId::Ui => vec![QueryFamily::Generic(GenericFamily::SansSerif)],
            FontId::Mono => {
                let mut out = Vec::new();
                if let Some(id) = self.mono_family {
                    out.push(QueryFamily::Id(id));
                }
                out.push(QueryFamily::Generic(GenericFamily::Monospace));
                out
            }
        }
    }

    /// Returns the first matching `QueryFont` for the requested style (MVP).
    pub fn resolve_first(&mut self, font_id: FontId, attrs: Attributes) -> Option<QueryFont> {
        let families = self.families_for(font_id);
        self.query_first(&families, attrs)
    }

    pub fn query_first(
        &mut self,
        families: &[QueryFamily<'static>],
        attrs: Attributes,
    ) -> Option<QueryFont> {
        let mut q = self.collection.query(&mut self.source_cache);
        q.set_families(families.iter().copied());
        q.set_attributes(attrs);
        // Minimal fallback: Latin script.
        q.set_fallbacks(FallbackKey::new(Script::from_bytes(*b"Latn"), None));

        let mut found: Option<QueryFont> = None;
        q.matches_with(|font| {
            found = Some(font.clone());
            QueryStatus::Stop
        });
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_mono_non_empty() {
        let mut db = FontDb::new();
        let f = db.resolve_first(FontId::Mono, Attributes::default());
        assert!(f.is_some());
    }

    #[test]
    fn fallback_queries_do_not_panic() {
        let mut db = FontDb::new();
        // Coverage is not guaranteed yet; resolution should not panic.
        let _ = db.resolve_first(FontId::Ui, Attributes::default());
    }
}
