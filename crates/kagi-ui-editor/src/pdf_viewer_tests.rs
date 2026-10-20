use super::*;
fn multipage(count: usize) -> PdfSource {
    multipage_sized(count, false)
}

fn multipage_sized(count: usize, short: bool) -> PdfSource {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Count {count} /Kids [{}] >>",
            (0..count)
                .map(|index| format!("{} 0 R", index + 3))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    for index in 0..count {
        let (width, height) = if short {
            (160, 8)
        } else if index % 2 == 0 {
            (612, 792)
        } else {
            (300, 400)
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] >>"
        ));
    }
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = data.len();
    data.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    PdfSource::from_bytes(data)
}

#[test]
fn continuous_geometry_reaches_every_mixed_size_page_without_count_clamping() {
    let source = multipage(111);
    assert_eq!(source.document.as_ref().unwrap().pages().len(), 111);
    let layout = PdfLayout::new(&source, 1.25, 16.);
    assert_eq!(layout.pages.len(), 111);
    assert_eq!(
        (layout.pages[0].width, layout.pages[0].height),
        (765., 990.)
    );
    assert_eq!(
        (layout.pages[1].width, layout.pages[1].height),
        (375., 500.)
    );
    for (index, page) in layout.pages.iter().enumerate() {
        assert_eq!(layout.page_at(page.top + page.height / 2.), index);
        assert!(layout.visible(page.top, 500.).contains(&index));
    }
    assert_eq!(
        layout.pages.last().unwrap().top + layout.pages.last().unwrap().height,
        layout.height
    );
    assert_eq!(layout.page_at(layout.height), 110);
}

#[test]
fn fit_uses_widest_finite_positive_page_and_live_padding_not_current_zoom() {
    let mut source = multipage(6);
    source.dimensions = Arc::from([
        (612., 792.),
        (3000., 2000.),
        (f32::INFINITY, 100.),
        (10000., f32::NAN),
        (20000., 0.),
        (-50000., 100.),
    ]);
    for current_zoom in [0.001, 0.25, 1., 4.] {
        let layout = PdfLayout::new(&source, current_zoom, 16.);
        assert_eq!(layout.fit_width_zoom(632., 16.), Some(0.2));
        assert_eq!(layout.fit_width_zoom(632., 32.), Some(568. / 3000.));
        assert_eq!(layout.fit_width_zoom(20000., 16.), Some(MAX_ZOOM));
        for width in [0., -1., 32., f32::NAN, f32::INFINITY] {
            assert_eq!(layout.fit_width_zoom(width, 16.), None);
        }
        for padding in [-1., f32::NAN, f32::INFINITY] {
            assert_eq!(layout.fit_width_zoom(632., padding), None);
        }
    }
    source.dimensions = Arc::from([(0., 100.), (100., f32::NAN)]);
    assert_eq!(
        PdfLayout::new(&source, 1., 16.).fit_width_zoom(632., 16.),
        None
    );
}

#[test]
fn fit_can_recover_oversized_geometry_and_keep_other_pages_subpixel() {
    let mut source = multipage(2);
    source.dimensions = Arc::from([(500000., 200000.), (160., 8.)]);
    let initial = PdfLayout::new(&source, 1., 16.);
    assert_eq!(initial.pages[0].error, Some(PdfError::Dimensions));
    let zoom = initial.fit_width_zoom(232., 16.).unwrap();
    assert!(zoom > 0. && zoom < MIN_ZOOM);
    let fitted = PdfLayout::new(&source, zoom, 16.);
    assert!((fitted.width - 200.).abs() < 0.001);
    assert!(fitted.pages.iter().all(|page| page.error.is_none()));
    assert!(fitted.pages[1].width < 1. && fitted.pages[1].height < 1.);
    assert_eq!(fitted.fit_width_zoom(232., 16.), Some(zoom));
}
#[test]
fn more_than_six_visible_short_pages_all_receive_actual_bounded_rasters() {
    let source = multipage_sized(24, true);
    let layout = PdfLayout::new(&source, 0.25, 16.);
    let visible = layout.visible(0., 400.);
    assert!(visible.len() > 6);
    let mut desired = Vec::new();
    layout.fill_desired(&source, 0, visible.clone(), (0.25, 2.), &mut desired);
    let mut total_bytes = 0;
    for page in visible {
        let demand = desired
            .iter()
            .find(|demand| demand.page == page)
            .expect("every visible page is demanded");
        let (image, _) = render_page_with_budget(
            source.document.as_ref().unwrap().clone(),
            page,
            0.25,
            2.,
            demand.pixel_budget,
        )
        .unwrap();
        let size = image.size(0);
        assert!(size.width.0 > 0 && size.height.0 > 0);
        total_bytes += size.width.0 as usize * size.height.0 as usize * 4;
    }
    assert!(total_bytes <= MAX_CACHED_BYTES);
}

#[test]
fn viewport_raster_demand_bounds_both_entries_and_bytes_at_each_density() {
    let source = multipage(111);
    let mut desired = Vec::new();
    for zoom in [0.25, 1., 4.] {
        let layout = PdfLayout::new(&source, zoom, 16.);
        for scale in [1., 2., 3.5] {
            for current in [0, 55, 110] {
                let visible = layout.visible(layout.pages[current].top, 10000.);
                layout.fill_desired(
                    &source,
                    current,
                    visible.clone(),
                    (zoom, scale),
                    &mut desired,
                );
                assert_eq!(desired.first().map(|demand| demand.page), Some(current));
                assert!(
                    desired.len() <= visible.len() + 2 && desired.len() < source.dimensions.len()
                );
                assert!(visible
                    .clone()
                    .all(|page| desired.iter().any(|demand| demand.page == page)));
                let bytes: usize = desired
                    .iter()
                    .map(|demand| {
                        let (w, h) = source.dimensions[demand.page];
                        let (x_scale, y_scale) =
                            crate::pdf::raster_scales(w, h, zoom, scale, demand.pixel_budget)
                                .unwrap();
                        (w * x_scale).floor() as usize * (h * y_scale).floor() as usize * 4
                    })
                    .sum();
                assert!(bytes <= MAX_CACHED_BYTES);
            }
        }
    }
}

#[test]
fn stale_rasters_reject_source_reopen_zoom_aba_density_and_departed_pages() {
    let source = multipage(3);
    let identity = RenderIdentity {
        generation: 7,
        source: source.bytes.clone(),
        page: 1,
        zoom: 1.25,
        backing_scale: 2.,
        pixel_budget: 2_000_000,
    };
    let desired = [
        RasterDemand {
            page: 1,
            pixel_budget: 2_000_000,
        },
        RasterDemand {
            page: 2,
            pixel_budget: 2_000_000,
        },
    ];
    assert!(identity.matches(&source, 7, 1.25, Some(2.), &desired));
    assert!(
        !identity.matches(&source, 9, 1.25, Some(2.), &desired),
        "zoom ABA is a newer generation"
    );
    assert!(!identity.matches(&source, 7, 1., Some(2.), &desired));
    assert!(!identity.matches(&source, 7, 1.25, Some(1.), &desired));
    assert!(!identity.matches(&source, 7, 1.25, None, &desired));
    assert!(!identity.matches(&source, 7, 1.25, Some(2.), &[]));
    assert!(
        !identity.matches(
            &source,
            7,
            1.25,
            Some(2.),
            &[RasterDemand {
                page: 1,
                pixel_budget: 1_000_000
            }]
        ),
        "viewport resolution revision rejects stale output"
    );
    assert!(
        !identity.matches(&multipage(3), 7, 1.25, Some(2.), &desired),
        "reopened equal bytes have different source ownership"
    );
}

#[test]
fn global_raster_admission_remains_bounded_until_publication_releases_its_permit() {
    let first = RasterPermit::acquire().unwrap();
    let second = RasterPermit::acquire().unwrap();
    assert!(RasterPermit::acquire().is_none());
    assert_eq!(RASTER_JOBS.load(Ordering::Acquire), MAX_RASTER_JOBS);
    drop(first);
    let replacement = RasterPermit::acquire().unwrap();
    assert!(RasterPermit::acquire().is_none());
    drop((second, replacement));
    assert_eq!(RASTER_JOBS.load(Ordering::Acquire), 0);
}
