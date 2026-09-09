//! [`FontDb`]: the bundled-fonts-only `fontique` collection.

use std::sync::Arc;

use fontique::{Blob, Collection, CollectionOptions, SourceCache, SourceCacheOptions};

use crate::bundled::{AHEM_BYTES, AHEM_FAMILY, NOTO_SANS_BYTES, NOTO_SANS_FAMILY};
use crate::error::FontError;

/// Opaque handle to one face in a [`FontDb`].
///
/// Stable for the lifetime of the `FontDb` that produced it (the database is
/// immutable after [`FontDb::bundled`] builds it), but not meaningful across
/// different `FontDb` instances -- always resolve keys through the same `FontDb`
/// you got them from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontKey(u32);

/// One bundled font face: a family name plus the raw font bytes and face index
/// `skrifa`/`swash`/`parley` need to parse and shape it.
#[derive(Debug, Clone, Copy)]
pub struct FontFace {
    /// This face's key in the [`FontDb`] it came from.
    pub key: FontKey,
    /// The face's family name, e.g. `"Ahem"` or `"Noto Sans"`.
    pub family: &'static str,
    /// The raw bytes of the font file containing this face (the whole
    /// `include_bytes!`-embedded asset, not a per-face slice).
    pub data: &'static [u8],
    /// This face's index within `data` (0 for a plain TTF/OTF; nonzero only for a
    /// font collection file, which neither bundled asset is today).
    pub index: u32,
}

/// The bundled font database: exactly two families (Ahem, Noto Sans), backed by a
/// `fontique` [`Collection`] that is constructed to never enumerate or read fonts
/// installed on the host system.
///
/// # Determinism
/// Every reftest and text golden in ChromeLight depends on rendering the same way
/// on every machine that runs it -- the owner's laptop and CI alike. That is only
/// true if text layout only ever sees these two fonts, never whatever happens to
/// be installed on the host. [`FontDb::bundled`] is the only constructor, and it
/// builds the underlying `fontique::Collection` with
/// `CollectionOptions { system_fonts: false, .. }`, which -- per `fontique`
/// 0.11's own implementation (`Inner::new`: `system_fonts.then(System::new)`) --
/// means the platform font-enumeration backend (`CoreText`/`DirectWrite`/
/// `fontconfig`) is never even constructed, let alone queried. Nothing in this
/// crate calls `Collection::load_system_fonts` either. The bundled bytes are
/// registered as in-memory blobs (`SourceKind::Memory`), so the `SourceCache`
/// never touches the filesystem.
pub struct FontDb {
    collection: Collection,
    source_cache: SourceCache,
    faces: Vec<FontFace>,
}

impl FontDb {
    /// Builds the bundled font database: Ahem and Noto Sans, and nothing else.
    ///
    /// See the [`FontDb`] docs for exactly how system font enumeration is
    /// disabled. Both assets are embedded via `include_bytes!` at compile time
    /// (see [`crate::bundled`]); nothing is read from disk at runtime.
    ///
    /// # Errors
    /// Returns [`FontError`] if the embedded bytes fail to register as fonts, or
    /// register under a family name other than the one this crate expects for
    /// that asset -- both would mean the build's bundled assets are corrupt or
    /// mismatched, not a normal runtime condition.
    pub fn bundled() -> Result<FontDb, FontError> {
        let mut collection = Collection::new(CollectionOptions {
            system_fonts: false,
            ..CollectionOptions::default()
        });
        let source_cache = SourceCache::new(SourceCacheOptions::default());

        let mut faces = Vec::with_capacity(2);
        register_bundled_face(&mut collection, AHEM_BYTES, AHEM_FAMILY, &mut faces)?;
        register_bundled_face(&mut collection, NOTO_SANS_BYTES, NOTO_SANS_FAMILY, &mut faces)?;

        Ok(FontDb {
            collection,
            source_cache,
            faces,
        })
    }

    /// Iterates over the family names present in this database.
    ///
    /// For [`FontDb::bundled`] this yields exactly `"Ahem"` and `"Noto Sans"`, in
    /// that order, once each.
    pub fn families(&self) -> impl Iterator<Item = &'static str> {
        self.faces.iter().map(|face| face.family)
    }

    /// Looks up a face by the key returned from [`FontDb::key_for`] or read off a
    /// [`FontFace`] from [`FontDb::families`]'s companion iteration. Returns
    /// `None` if `key` did not come from this database.
    pub fn face(&self, key: FontKey) -> Option<&FontFace> {
        self.faces.get(key.0 as usize)
    }

    /// Resolves a CSS `font-family` value to a bundled face.
    ///
    /// Matches the family's exact bundled name first (`"Ahem"`, `"Noto Sans"`).
    /// Failing that, the M1a generic-family subset maps both `sans-serif` and
    /// `monospace` to Noto Sans -- ChromeLight does not bundle a distinct
    /// monospace face yet, so the generic keyword still resolves to a real,
    /// deterministic face rather than falling through. Any other name (a system
    /// font, `serif`, `cursive`, `fantasy`, an unbundled web font) returns `None`;
    /// there is nothing else this crate can hand back without reading the host.
    pub fn key_for(&self, family: &str) -> Option<FontKey> {
        if let Some(face) = self.faces.iter().find(|face| face.family == family) {
            return Some(face.key);
        }
        match family {
            "sans-serif" | "monospace" => self
                .faces
                .iter()
                .find(|face| face.family == NOTO_SANS_FAMILY)
                .map(|face| face.key),
            _ => None,
        }
    }

    /// Borrows the underlying `fontique` collection and source cache, for
    /// building a `parley::FontContext` (Task 18's text-shaping pipeline) without
    /// this crate needing to depend on `parley` itself.
    pub fn fontique(&mut self) -> (&mut Collection, &mut SourceCache) {
        (&mut self.collection, &mut self.source_cache)
    }
}

/// Registers one bundled asset's bytes with `collection`, checks that it
/// registered as a single family under `expected_family`, and appends a
/// [`FontFace`] to `faces` for every face `fontique` found in it.
fn register_bundled_face(
    collection: &mut Collection,
    data: &'static [u8],
    expected_family: &'static str,
    faces: &mut Vec<FontFace>,
) -> Result<(), FontError> {
    let blob = Blob::new(Arc::new(data));
    let registered = collection.register_fonts(blob, None);

    if registered.is_empty() {
        return Err(FontError::NoFacesRegistered {
            name: expected_family,
        });
    }
    if registered.len() != 1 {
        return Err(FontError::UnexpectedFamilyCount {
            name: expected_family,
            count: registered.len(),
        });
    }

    let (family_id, font_infos) = &registered[0];
    let actual_name = collection
        .family_name(*family_id)
        .map(str::to_owned)
        .unwrap_or_default();
    if actual_name != expected_family {
        return Err(FontError::UnexpectedFamilyName {
            expected: expected_family,
            actual: actual_name,
        });
    }

    for info in font_infos {
        let key = FontKey(u32::try_from(faces.len()).unwrap_or(u32::MAX));
        faces.push(FontFace {
            key,
            family: expected_family,
            data,
            index: info.index(),
        });
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const KNOWN_SYSTEM_FAMILIES: &[&str] = &[
        "Helvetica",
        "Arial",
        "DejaVu Sans",
        "Segoe UI",
        "Liberation Sans",
    ];

    #[test]
    fn bundled_db_should_contain_exactly_two_families() {
        let db = FontDb::bundled().expect("bundled db builds");
        let families: Vec<&str> = db.families().collect();
        assert_eq!(families, vec![AHEM_FAMILY, NOTO_SANS_FAMILY]);
        assert_eq!(db.families().count(), 2);
    }

    #[test]
    fn bundled_db_should_not_read_system_fonts() {
        let mut db = FontDb::bundled().expect("bundled db builds");
        let families: Vec<&str> = db.families().collect();
        assert_eq!(
            families,
            vec![AHEM_FAMILY, NOTO_SANS_FAMILY],
            "family list must be exactly the two bundled names, in order"
        );
        for system_family in KNOWN_SYSTEM_FAMILIES {
            assert!(
                !families.contains(system_family),
                "bundled db must never contain a system family, found {system_family}"
            );
        }

        // The check above only exercises this crate's own bookkeeping (`self.faces`),
        // which by construction only ever contains what `bundled()` explicitly
        // registered -- it would stay "clean" even if system font loading were
        // silently switched on, since `faces` is never populated from `Collection`'s
        // system source. The property that actually matters is that the underlying
        // `fontique::Collection` itself never merged in a system source, so assert
        // directly against `Collection::family_names`, which is exactly what would
        // grow to include `Helvetica`/`Arial`/etc. if `CollectionOptions::system_fonts`
        // were ever flipped to `true` (see `fontique`'s own `Inner::family_names`,
        // which chains `self.data.family_names` with `self.system.family_names` only
        // when `self.system` is `Some`).
        let (collection, _source_cache) = db.fontique();
        let collection_families: Vec<String> = collection.family_names().map(String::from).collect();
        assert_eq!(
            collection_families.len(),
            2,
            "fontique::Collection must know about exactly the two bundled families, \
             found {collection_families:?} -- this would grow if system font loading \
             were ever switched on"
        );
        for system_family in KNOWN_SYSTEM_FAMILIES {
            assert!(
                !collection_families.iter().any(|f| f == system_family),
                "fontique::Collection must never contain a system family, found {system_family}"
            );
        }
    }

    #[test]
    fn ahem_should_map_from_generic_alias() {
        let db = FontDb::bundled().expect("bundled db builds");
        let key = db.key_for("Ahem").expect("Ahem resolves");
        let face = db.face(key).expect("key resolves to a face");
        assert_eq!(face.family, AHEM_FAMILY);
    }

    #[test]
    fn sans_serif_should_map_to_noto() {
        let db = FontDb::bundled().expect("bundled db builds");
        let sans_key = db.key_for("sans-serif").expect("sans-serif resolves");
        let mono_key = db.key_for("monospace").expect("monospace resolves");
        let sans_face = db.face(sans_key).expect("key resolves to a face");
        let mono_face = db.face(mono_key).expect("key resolves to a face");
        assert_eq!(sans_face.family, NOTO_SANS_FAMILY);
        assert_eq!(mono_face.family, NOTO_SANS_FAMILY);
    }

    #[test]
    fn face_should_return_none_for_unknown_key() {
        let db = FontDb::bundled().expect("bundled db builds");
        // Two faces are registered (indices 0, 1); index 2 names no face.
        let bogus_key = FontKey(2);
        assert!(db.face(bogus_key).is_none());
    }

    #[test]
    fn key_for_should_return_none_for_unknown_family() {
        let db = FontDb::bundled().expect("bundled db builds");
        assert!(db.key_for("Comic Sans MS").is_none());
        assert!(db.key_for("serif").is_none());
    }

    #[test]
    fn fontique_should_expose_collection_and_source_cache() {
        let mut db = FontDb::bundled().expect("bundled db builds");
        let (collection, _source_cache) = db.fontique();
        // The collection must know about both bundled families by name, confirming
        // the borrowed handle really is the same collection `bundled()` populated.
        assert!(collection.family_id(AHEM_FAMILY).is_some());
        assert!(collection.family_id(NOTO_SANS_FAMILY).is_some());
    }

    /// The defining property of Ahem (https://web-platform-tests.org, and the W3C
    /// CSS test suite before it): every glyph is a solid box exactly one em
    /// square, so the advance width of any mapped character equals the font's
    /// `unitsPerEm`. This is what actually proves the embedded bytes are Ahem and
    /// not some other font that happens to register under the name "Ahem" --
    /// unlike the family-name checks elsewhere in this module, this test parses
    /// the real glyph outlines/metrics via `skrifa`.
    #[test]
    fn ahem_glyph_should_be_square_em() {
        use skrifa::instance::{LocationRef, Size};
        use skrifa::{FontRef, MetadataProvider};

        let font = FontRef::new(AHEM_BYTES).expect("Ahem bytes parse as a font");
        let units_per_em = font.metrics(Size::unscaled(), LocationRef::default()).units_per_em;
        let glyph_id = font
            .charmap()
            .map('x')
            .expect("Ahem's cmap must map 'x' to a glyph");
        let advance = font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(glyph_id)
            .expect("advance width must be available for 'x'");

        assert_eq!(advance, f32::from(units_per_em));
    }
}
