use super::*;

const MULTIPAGE: &[u8] = include_bytes!("../../../tests/fixtures/pdf/document.pdf");
const JAPANESE_FALLBACK: &[u8] =
    include_bytes!("../../../tests/fixtures/pdf/japanese-fallback.pdf");
// FontTools outlines from the bundled Noto Unicode cmap, independent of PDF
// font selection and CID mapping: 日=6369, 本=6605, 語=12177 (1000 UPEM).
const JAPANESE_OUTLINES: &[u8] =
    include_bytes!("../../../tests/fixtures/pdf/japanese-outlines.pdf");
const PASSWORD: &[u8] = include_bytes!("../../../tests/fixtures/pdf/password.pdf");
const ROTATED: &[u8] = include_bytes!("../../../tests/fixtures/pdf/rotated.pdf");

fn source(bytes: &[u8]) -> PdfSource {
    PdfSource::from_bytes(bytes.to_vec())
}
fn no_image(_: Vec<u8>) -> Option<Arc<gpui::Image>> {
    None
}

#[test]
fn pdf_classification_precedes_binary_and_utf8_and_is_not_validity() {
    assert!(recognizes_pdf(
        Path::new("uppercase.PdF"),
        b"editable-looking garbage"
    ));
    assert!(recognizes_pdf(
        Path::new("no-extension"),
        b"%PDF-1.7\nASCII-only"
    ));
    assert!(!recognizes_pdf(Path::new("notes.txt"), b"not a PDF"));
    // A real all-ASCII PDF must render, not become a source editor.
    assert!(ROTATED.iter().all(u8::is_ascii));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.PDF");
    std::fs::write(&path, b"not a PDF, not binary, valid UTF-8").unwrap();
    let loaded = read_file(&path, no_image, None);
    assert!(loaded.text.is_none());
    assert!(loaded.is_binary);
    assert_eq!(loaded.pdf.unwrap().document.err(), Some(PdfError::Invalid));
    std::fs::write(&path, ROTATED).unwrap();
    let loaded = read_file(&path, no_image, None);
    assert!(loaded.text.is_none());
    assert_eq!(loaded.pdf.unwrap().document.unwrap().pages().len(), 1);
    let misnamed = dir.path().join("signature.txt");
    std::fs::write(&misnamed, ROTATED).unwrap();
    let loaded = read_file(&misnamed, no_image, None);
    assert!(loaded.text.is_none());
    assert_eq!(loaded.pdf.unwrap().document.unwrap().pages().len(), 1);
}

#[test]
fn regular_file_byte_gate_remains_ten_mib_and_read_is_hard_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.pdf");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_EDITOR_BYTES + 1).unwrap();
    let loaded = read_file(&path, no_image, None);
    assert!(loaded.too_large);
    assert!(loaded.pdf.is_none());
    assert!(loaded.text.is_none());
    assert!(read_file(dir.path(), no_image, None).missing);
    assert!(read_file(&dir.path().join("missing.pdf"), no_image, None).missing);
    let oversized = PdfSource::from_bytes(vec![0; MAX_EDITOR_BYTES as usize + 1]);
    assert_eq!(oversized.document.err(), Some(PdfError::TooLarge));
    assert!(
        oversized.bytes.is_empty(),
        "rejected source releases the oversized allocation"
    );
}

#[test]
fn parser_password_and_encryption_errors_are_not_reported_as_binary_or_all_corrupt() {
    assert_eq!(source(PASSWORD).document.err(), Some(PdfError::Password));
    assert_eq!(PdfError::from(LoadPdfError::Invalid), PdfError::Invalid);
    assert_eq!(
        PdfError::from(LoadPdfError::Decryption(
            DecryptionError::UnsupportedAlgorithm
        )),
        PdfError::Encryption
    );
    for error in [
        DecryptionError::MissingIDEntry,
        DecryptionError::InvalidEncryption,
    ] {
        assert_eq!(
            PdfError::from(LoadPdfError::Decryption(error)),
            PdfError::Invalid
        );
    }
}

#[test]
fn raster_dimensions_are_validated_before_hayro_u16_conversion() {
    for (width, height, zoom) in [
        (f32::NAN, 100., 1.),
        (100., f32::INFINITY, 1.),
        (0., 100., 1.),
        (-1., 100., 1.),
        (1e20, 100., 1.),
        (100., 100., f32::NAN),
        (100., 100., 0.),
        (100., 100., 5.),
        (0.1, 100., 1.),
    ] {
        assert_eq!(
            raster_scale(width, height, zoom, 1.),
            Err(PdfError::Dimensions)
        );
    }
    for backing_scale in [0., -1., f32::NAN, f32::INFINITY] {
        assert_eq!(
            raster_scale(612., 792., 1., backing_scale),
            Err(PdfError::Dimensions)
        );
    }
    for (width, height, zoom) in [
        (612., 792., 1.),
        (612., 792., 4.),
        (10000., 10000., 1.),
        (20000., 5000., 4.),
    ] {
        for backing_scale in [1., 2., 3.5] {
            let scale = raster_scale(width, height, zoom, backing_scale).unwrap();
            let (w, h) = ((width * scale) as u64, (height * scale) as u64);
            assert!(w > 0 && h > 0 && w <= MAX_SIDE as u64 && h <= MAX_SIDE as u64);
            assert!(w * h <= MAX_PIXELS as u64);
        }
    }
}

#[test]
fn viewport_pixel_budgets_render_every_page_without_exceeding_its_share() {
    for (width, height) in [(612., 792.), (160., 8.), (4096., 4096.)] {
        for budget in [1, 64, 1000, 1_000_000, MAX_PIXELS as usize] {
            let (x_scale, y_scale) = raster_scales(width, height, 1., 2., budget).unwrap();
            let (w, h) = (
                (width * x_scale).floor() as usize,
                (height * y_scale).floor() as usize,
            );
            assert!(w >= 1 && h >= 1);
            assert!(w <= MAX_SIDE as usize && h <= MAX_SIDE as usize);
            assert!(w * h <= budget);
        }
    }
    assert_eq!(
        raster_scales(612., 792., 1., 2., 0),
        Err(PdfError::Dimensions)
    );
}

#[test]
fn unchanged_reads_reuse_parsed_document_and_replacements_use_new_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("document.pdf");
    std::fs::write(&path, MULTIPAGE).unwrap();
    let old = read_file(&path, no_image, None).pdf.unwrap();
    let unchanged = read_file(&path, no_image, Some(old.clone())).pdf.unwrap();
    assert!(Arc::ptr_eq(&old.bytes, &unchanged.bytes));
    assert!(Arc::ptr_eq(
        old.document.as_ref().unwrap(),
        unchanged.document.as_ref().unwrap()
    ));
    std::fs::write(&path, JAPANESE_FALLBACK).unwrap();
    let replacement = read_file(&path, no_image, Some(old.clone())).pdf.unwrap();
    assert!(!Arc::ptr_eq(&old.bytes, &replacement.bytes));
    assert_eq!(replacement.dimensions.len(), 1);
    assert_eq!(replacement.dimensions[0], (300., 300.));
}

#[test]
fn actual_pdf_pixels_navigation_zoom_rotation_and_japanese_font_fallback() {
    let document = source(MULTIPAGE).document.unwrap();
    assert_eq!(document.pages().len(), 3);
    let (first, _) = render_page(document.clone(), 0, 1., 1.).unwrap();
    let (second, _) = render_page(document.clone(), 1, 1., 1.).unwrap();
    let bytes = first.as_bytes(0).unwrap();
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|p| p[0] < 80 && p[1] < 80 && p[2] < 80)
            .count()
            > 200,
        "actual dark glyph/graphic pixels, not a blank allocated image"
    );
    assert_ne!(
        bytes,
        second.as_bytes(0).unwrap(),
        "distinct page content must be rendered"
    );
    let (retina, _) = render_page(document.clone(), 0, 1., 2.).unwrap();
    assert_eq!(
        retina.size(0),
        gpui::size(gpui::DevicePixels(1224), gpui::DevicePixels(1584))
    );
    let (zoomed, _) = render_page(document, 1, 1.25, 1.).unwrap();
    assert!(zoomed.size(0).width > second.size(0).width);
    assert!(zoomed.size(0).height > second.size(0).height);
    let rotated = source(ROTATED).document.unwrap();
    assert_eq!(
        rotated.pages().get(0).unwrap().render_dimensions(),
        (700., 500.)
    );
    let (rotation, _) = render_page(rotated, 0, 1., 1.).unwrap();
    assert_eq!(
        rotation.size(0),
        gpui::size(gpui::DevicePixels(700), gpui::DevicePixels(500))
    );
    let (japanese, partial) =
        render_page(source(JAPANESE_FALLBACK).document.unwrap(), 0, 1., 1.).unwrap();
    assert!(!partial, "supported AdobeJapan1 Japanese fallback fixture");
    let (outlines, outline_partial) =
        render_page(source(JAPANESE_OUTLINES).document.unwrap(), 0, 1., 1.).unwrap();
    assert!(!outline_partial);
    assert_eq!(japanese.size(0), outlines.size(0));
    let bytes = japanese.as_bytes(0).unwrap();
    let expected = outlines.as_bytes(0).unwrap();
    for (index, character) in ['日', '本', '語'].into_iter().enumerate() {
        let (mut intersection, mut union) = (0usize, 0usize);
        for y in 65..110 {
            for x in (40 + index * 32)..(72 + index * 32) {
                let offset = (y * 300 + x) * 4;
                let actual_ink = bytes[offset..offset + 3]
                    .iter()
                    .all(|&channel| channel < 128);
                let expected_ink = expected[offset..offset + 3]
                    .iter()
                    .all(|&channel| channel < 128);
                intersection += usize::from(actual_ink && expected_ink);
                union += usize::from(actual_ink || expected_ink);
            }
        }
        assert!(
            intersection > 50 && intersection * 100 >= union * 94,
            "{character} must match its independent Noto outline: intersection={intersection}, union={union}"
        );
    }
    assert_eq!(
        &bytes[..4],
        &[255, 255, 255, 255],
        "opaque white page background"
    );
    let green = &bytes[(260 * 300 + 40) * 4..(260 * 300 + 40) * 4 + 4];
    assert!(
        green[0] > 50 && green[0] < 110 && green[1] > 150 && green[2] < 60 && green[3] == 255,
        "known PDF RGB(0.1,0.7,0.3) rectangle must arrive as BGRA: {green:?}"
    );
}

#[test]
fn an_empty_page_tree_is_a_truthful_invalid_document_not_a_stuck_loading_state() {
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [] /Count 0 >>",
    ]
    .into_iter()
    .enumerate()
    {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = data.len();
    data.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
    for offset in &offsets[1..] {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    assert!(
        Pdf::new(data.clone()).unwrap().pages().is_empty(),
        "genuine empty PDF page tree"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.pdf");
    std::fs::write(&path, data).unwrap();
    let loaded = read_file(&path, no_image, None);
    assert_eq!(loaded.pdf.unwrap().document.err(), Some(PdfError::Invalid));
}
