//! Fontique-backed discovery for bundled and operating-system fonts.

use ailloli_ui_core::FontId;
use fontique::{
    Attributes, Blob, Collection, CollectionOptions, FallbackKey, GenericFamily, QueryFamily,
    QueryFont, QueryStatus, Script, SourceCache, SourceCacheOptions,
};

/// Bundled font bytes that guarantee a monospace-family candidate.
const JBM_NERD_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Font database via `fontique`: bundled assets + system fonts for Parley layout.
///
/// The database owns its discovery collection and mutable source cache. Clones
/// have cloneable Fontique state but are otherwise independent values. System
/// font contents and ordering are platform-dependent; the bundled monospace
/// face is registered first to provide a deterministic candidate for
/// [`FontId::Mono`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_text::FontDb;
/// let db = FontDb::new();
/// assert_eq!(db.families_for(FontId::Ui).len(), 1);
/// assert!(db.families_for(FontId::Mono).len() >= 1);
/// ```
#[derive(Clone)]
pub struct FontDb {
    /// Bundled and system-font collection used for matching.
    collection: Collection,
    /// Cache for loading font sources selected by collection queries.
    source_cache: SourceCache,
    /// Registered family identifier for the bundled monospace face.
    mono_family: Option<fontique::FamilyId>,
}

impl Default for FontDb {
    fn default() -> Self {
        Self::new()
    }
}

impl FontDb {
    /// Builds a database with system discovery and bundled fonts enabled.
    ///
    /// Construction registers JetBrains Mono Nerd Font, scans the platform's
    /// font sources through Fontique, and optionally scans the relative
    /// `assets/fonts` directory. Invalid optional paths are ignored. System
    /// discovery may perform filesystem I/O and its results vary by platform.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::FontDb;
    /// let db = FontDb::new();
    /// assert!(db.families_for(FontId::Mono).len() >= 1);
    /// ```
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

    /// Returns the ordered query families for an Ailloli font slot.
    ///
    /// [`FontId::Ui`] returns only generic sans-serif. [`FontId::Mono`] returns
    /// the bundled family first when registration succeeded, then generic
    /// monospace. The newly allocated vector is suitable for [`Self::query_first`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::FontDb;
    /// let db = FontDb::new();
    /// assert_eq!(db.families_for(FontId::Ui).len(), 1);
    /// assert!(db.families_for(FontId::Mono).len() >= 1);
    /// ```
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

    /// Returns the first matching `QueryFont` for the requested style.
    ///
    /// Matching uses the ordered families from [`Self::families_for`], the
    /// supplied weight/stretch/style attributes, and a Latin-script fallback.
    /// It stops after the first result. `None` means Fontique found no candidate;
    /// system-font availability makes that outcome platform-dependent for UI.
    /// The mutable borrow updates Fontique's source cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::FontDb;
    /// use fontique::Attributes;
    /// let mut db = FontDb::new();
    /// assert!(db.resolve_first(FontId::Mono, Attributes::default()).is_some());
    /// ```
    pub fn resolve_first(&mut self, font_id: FontId, attrs: Attributes) -> Option<QueryFont> {
        let families = self.families_for(font_id);
        self.query_first(&families, attrs)
    }

    /// Returns the first Fontique match for explicit ordered families.
    ///
    /// The query always installs a Latin (`Latn`) fallback key and stops after
    /// its first callback. An empty or unmatched family slice may still resolve
    /// through fallback; callers must treat `None` as a normal miss. The result
    /// borrows no data from `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::FontId;
    /// use ailloli_ui_text::FontDb;
    /// use fontique::Attributes;
    /// let mut db = FontDb::new();
    /// let families = db.families_for(FontId::Mono);
    /// assert!(db.query_first(&families, Attributes::default()).is_some());
    /// ```
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
