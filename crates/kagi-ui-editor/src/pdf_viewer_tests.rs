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
                    desired.len() <= visible.len() + 3 && desired.len() < source.dimensions.len()
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
fn native_density_survives_a_neighbour_entering_and_prefetched_preview_is_reusable() {
    let source = multipage(12);
    let layout = PdfLayout::new(&source, 1., 16.);
    let mut before = Vec::new();
    let mut after = Vec::new();
    layout.fill_desired(&source, 4, 4..5, (1., 2.), &mut before);
    layout.fill_desired(&source, 4, 4..6, (1., 2.), &mut after);
    assert_eq!(
        before[0], after[0],
        "entering page must not change page 4's raster key"
    );
    let prefetched = before.iter().find(|demand| demand.page == 5).unwrap();
    assert!(prefetched.pixel_budget <= PREVIEW_PIXELS);
    assert!(before.iter().any(|demand| demand.page == 3));
    assert!(before.iter().any(|demand| demand.page == 6));
    let identity = RenderIdentity {
        generation: 7,
        source: source.bytes.clone(),
        page: prefetched.page,
        zoom: 1.,
        backing_scale: 2.,
        pixel_budget: prefetched.pixel_budget,
    };
    assert!(identity.matches(&source, 7, 1., Some(2.), &after));
}

#[test]
fn settled_zoom_retains_native_quality_when_the_visible_working_set_fits() {
    let source = multipage(8);
    let mut desired = Vec::new();
    for zoom in [2., 4.] {
        let layout = PdfLayout::new(&source, zoom, 16.);
        layout.fill_desired(&source, 2, 2..3, (zoom, 2.), &mut desired);
        let (width, height) = source.dimensions[2];
        let scale = raster_scale(width, height, zoom, 2.).unwrap();
        let native = (width * scale).ceil() as usize * (height * scale).ceil() as usize;
        assert_eq!(desired[0].pixel_budget, native.min(MAX_PAGE_PIXELS));
        assert!(desired[0].pixel_budget > 4_000_000);
        assert!(
            desired
                .iter()
                .map(|demand| demand.pixel_budget)
                .sum::<usize>()
                <= MAX_CACHED_PIXELS,
            "full-quality visible demand plus all prefetch must fit",
        );
        // The narrower next page does not force equal-share allocation or a
        // quality change while the native demands still fit together.
        let (width, height) = source.dimensions[3];
        let scale = raster_scale(width, height, zoom, 2.).unwrap();
        let neighbour_native = (width * scale).ceil() as usize * (height * scale).ceil() as usize;
        if native + neighbour_native <= MAX_CACHED_PIXELS - 3 * PREVIEW_PIXELS {
            let previous = desired[0];
            layout.fill_desired(&source, 2, 2..4, (zoom, 2.), &mut desired);
            assert_eq!(desired[0], previous);
            assert_eq!(desired[1].pixel_budget, neighbour_native);
        }
    }
    let single = multipage(1);
    let layout = PdfLayout::new(&single, 4., 16.);
    layout.fill_desired(&single, 0, 0..1, (4., 2.), &mut desired);
    let (width, height) = single.dimensions[0];
    let scale = raster_scale(width, height, 4., 2.).unwrap();
    let native = (width * scale).ceil() as usize * (height * scale).ceil() as usize;
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].pixel_budget, native.min(MAX_CACHED_PIXELS));
}

fn cached_page(source: &PdfSource, demand: RasterDemand, generation: u64) -> CachedPage {
    let (image, partial) = render_page_with_budget(
        source.document.as_ref().unwrap().clone(),
        demand.page,
        4.,
        2.,
        demand.pixel_budget,
    )
    .unwrap();
    let size = image.size(0);
    CachedPage {
        page: demand.page,
        generation,
        pixel_budget: demand.pixel_budget,
        bytes: size.width.0 as usize * size.height.0 as usize * 4,
        image: Some(image),
        error: None,
        partial,
    }
}

#[test]
fn cold_visible_pages_precede_refinement_and_prefetch_then_return_hits_reuse_the_image() {
    let source = multipage(12);
    let desired = [
        RasterDemand {
            page: 4,
            pixel_budget: 2_000_000,
        },
        RasterDemand {
            page: 5,
            pixel_budget: 2_000_000,
        },
        RasterDemand {
            page: 6,
            pixel_budget: PREVIEW_PIXELS,
        },
    ];
    let mut cache = VecDeque::new();
    let first = next_raster(&desired, &(4..6), &cache, 7, true).unwrap();
    assert_eq!(
        first,
        RasterDemand {
            page: 4,
            pixel_budget: PREVIEW_PIXELS
        }
    );
    insert_cached_page(
        &mut cache,
        cached_page(&source, first, 7),
        &desired,
        7,
        |_| {},
    );
    let second = next_raster(&desired, &(4..6), &cache, 7, true).unwrap();
    assert_eq!(
        second,
        RasterDemand {
            page: 5,
            pixel_budget: PREVIEW_PIXELS
        }
    );
    insert_cached_page(
        &mut cache,
        cached_page(&source, second, 7),
        &desired,
        7,
        |_| {},
    );
    assert_eq!(
        next_raster(&desired, &(4..6), &cache, 7, false),
        Some(desired[2]),
        "motion permits useful previews but not expensive refinements",
    );
    assert_eq!(
        next_raster(&desired, &(4..6), &cache, 7, true),
        Some(desired[0])
    );
    insert_cached_page(
        &mut cache,
        cached_page(&source, desired[0], 7),
        &desired,
        7,
        |_| {},
    );
    let retained = cache
        .iter()
        .find(|entry| entry.page == 4)
        .unwrap()
        .image
        .clone()
        .unwrap();
    let away = [RasterDemand {
        page: 10,
        pixel_budget: PREVIEW_PIXELS,
    }];
    insert_cached_page(
        &mut cache,
        cached_page(&source, away[0], 7),
        &away,
        7,
        |_| {},
    );
    assert!(
        cache.iter().any(|entry| entry.page == 4),
        "departure is not eviction"
    );
    assert!(next_raster(&desired[..1], &(4..5), &cache, 7, true).is_none());
    assert!(Arc::ptr_eq(
        &retained,
        cache
            .iter()
            .find(|entry| entry.page == 4)
            .unwrap()
            .image
            .as_ref()
            .unwrap(),
    ));
}

#[test]
fn actual_rasters_obey_lru_byte_and_entry_caps_and_retire_only_removed_images() {
    let source = multipage(48);
    let mut cache = VecDeque::new();
    let mut retired = Vec::new();
    for page in 0..12 {
        let demand = RasterDemand {
            page,
            pixel_budget: 4_000_000,
        };
        let entry = cached_page(&source, demand, 1);
        insert_cached_page(&mut cache, entry, &[demand], 1, |image| retired.push(image));
        assert!(cache.iter().map(|entry| entry.bytes).sum::<usize>() <= MAX_CACHED_BYTES);
        assert!(cache.front().unwrap().page == page);
        assert!(retired.iter().all(|image| cache.iter().all(|entry| {
            entry
                .image
                .as_ref()
                .is_none_or(|current| !Arc::ptr_eq(image, current))
        })));
        retired.clear();
    }
    assert!(
        cache.len() < 12,
        "actual large rasters exercise byte eviction"
    );
    for page in 12..48 {
        let demand = RasterDemand {
            page,
            pixel_budget: 64,
        };
        insert_cached_page(
            &mut cache,
            cached_page(&source, demand, 1),
            &[demand],
            1,
            |_| {},
        );
        assert!(cache.len() <= RETAINED_PAGE_ENTRIES);
        assert!(cache.iter().map(|entry| entry.bytes).sum::<usize>() <= MAX_CACHED_BYTES);
    }
    assert!(
        !cache.iter().any(|entry| entry.page == 12),
        "oldest return entry is evicted"
    );
    assert!(cache.iter().any(|entry| entry.page == 47));
}

#[test]
fn visible_short_pages_above_return_entry_cap_all_settle_without_raster_eviction_loops() {
    let source = multipage_sized(80, true);
    let layout = PdfLayout::new(&source, 0.001, 1.);
    let mut visible = layout.visible(0., 60.);
    assert!(visible.len() > RETAINED_PAGE_ENTRIES);
    let mut desired = Vec::new();
    layout.fill_desired(&source, 0, visible.clone(), (0.001, 2.), &mut desired);
    let mut cache = VecDeque::new();
    let mut renders = 0;
    while let Some(demand) = next_raster(&desired, &visible, &cache, 1, true) {
        insert_cached_page(
            &mut cache,
            cached_page(&source, demand, 1),
            &desired,
            1,
            |_| panic!("all current tiny demands must fit"),
        );
        renders += 1;
        assert!(renders <= desired.len(), "each demand renders once");
    }
    assert_eq!(renders, desired.len());
    assert!(visible.all(|page| cache.iter().any(|entry| entry.page == page)));
    assert!(cache.iter().map(|entry| entry.bytes).sum::<usize>() <= MAX_CACHED_BYTES);
}

#[test]
fn queued_obsolete_demand_skips_renderer_and_a_late_preview_cannot_satisfy_new_density() {
    let revision = AtomicU64::new(1);
    let calls = AtomicUsize::new(0);
    revision.store(2, Ordering::Release);
    assert!(render_if_current(&revision, 1, || calls.fetch_add(1, Ordering::Relaxed)).is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        render_if_current(&revision, 2, || calls.fetch_add(1, Ordering::Relaxed)),
        Some(0)
    );
    let source = multipage(3);
    let demand = RasterDemand {
        page: 1,
        pixel_budget: 2_000_000,
    };
    let entry = cached_page(
        &source,
        RasterDemand {
            page: 1,
            pixel_budget: PREVIEW_PIXELS,
        },
        7,
    );
    assert!(
        !entry.satisfies(demand, 7),
        "a preview remains usable but needs refinement"
    );
    assert!(!entry.satisfies(
        RasterDemand {
            page: 1,
            pixel_budget: PREVIEW_PIXELS
        },
        8,
    ));
    assert!(!entry.satisfies(
        RasterDemand {
            page: 2,
            pixel_budget: PREVIEW_PIXELS
        },
        7,
    ));
}

#[cfg(feature = "gui-e2e")]
#[gpui::test]
fn viewer_publication_rejects_wrong_renderer_identity_and_retains_better_image(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::AppContext as _;
    let source = multipage(3);
    let reopened = multipage(3);
    let viewer = cx.new(|cx| PdfViewer::new(source.clone(), cx));
    let demand = RasterDemand {
        page: 0,
        pixel_budget: 2_000_000,
    };
    let document = source.document.as_ref().unwrap();
    let full = render_page_with_budget(document.clone(), 0, 1., 2., demand.pixel_budget)
        .unwrap()
        .0;
    let preview = render_page_with_budget(document.clone(), 0, 1., 2., PREVIEW_PIXELS)
        .unwrap()
        .0;
    viewer.update(cx, |viewer, cx| {
        viewer.generation = 7;
        viewer.backing_scale = Some(2.);
        viewer.desired = vec![demand];
        let identity =
            |source: &PdfSource, generation, page, backing_scale, pixel_budget| RenderIdentity {
                generation,
                source: source.bytes.clone(),
                page,
                zoom: 1.,
                backing_scale,
                pixel_budget,
            };
        viewer.publish_raster(
            identity(&source, 7, 0, 2., demand.pixel_budget),
            Ok((full.clone(), false)),
            cx,
        );
        assert!(Arc::ptr_eq(viewer.cache[0].image.as_ref().unwrap(), &full));
        for stale in [
            identity(&source, 7, 0, 2., PREVIEW_PIXELS),
            identity(&source, 7, 1, 2., PREVIEW_PIXELS),
            identity(&source, 7, 0, 1., PREVIEW_PIXELS),
            identity(&source, 5, 0, 2., PREVIEW_PIXELS),
            identity(&reopened, 7, 0, 2., PREVIEW_PIXELS),
        ] {
            viewer.publish_raster(stale, Ok((preview.clone(), false)), cx);
            assert_eq!(viewer.cache.len(), 1);
            assert!(Arc::ptr_eq(viewer.cache[0].image.as_ref().unwrap(), &full));
        }
        viewer.replace_source(reopened.clone(), cx);
        viewer.publish_raster(
            identity(&source, 7, 0, 2., demand.pixel_budget),
            Ok((full.clone(), false)),
            cx,
        );
        assert!(viewer.cache.is_empty());
        assert!(Arc::ptr_eq(&viewer.source.bytes, &reopened.bytes));
    });
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
