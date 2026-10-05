//! Reusable native-window inventory (#1047), compiled only by gui-e2e.
mod capture;
mod composition;
mod contracts;
mod fixtures;
mod modals;
mod popups;
mod table;

use fixtures::{Fixture, FixtureKind};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;

type Open = fn(&Fixture, &Entity<KagiApp>, AnyWindowHandle, &mut VisualTestAppContext);
enum Action {
    Capture(Open),
    Skip(&'static str),
}
struct Entry {
    name: &'static str,
    description: &'static str,
    fixture: FixtureKind,
    action: Action,
}

pub(crate) fn register(
    scenarios: &mut Vec<crate::macos::Scenario>,
    requested: bool,
    filters: Option<&[String]>,
    exact: Option<&str>,
) {
    if requested {
        scenarios.extend(table::INVENTORY.iter().map(|entry| {
            (
                entry.name,
                Box::new(move |cx: &mut VisualTestAppContext| capture::run(entry, cx))
                    as Box<dyn FnMut(&mut VisualTestAppContext)>,
            )
        }));
    }
    let contracts_requested = exact.map_or_else(
        || {
            filters.is_some_and(|filters| {
                filters
                    .iter()
                    .any(|filter| filter.starts_with("inventory_tool_"))
            })
        },
        |name| name.starts_with("inventory_tool_"),
    );
    if !contracts_requested {
        return;
    }
    scenarios.push((
        "inventory_tool_composition",
        Box::new(composition::scenario_inventory_composition),
    ));
    scenarios.extend([
        (
            "inventory_tool_selection",
            Box::new(contracts::scenario_inventory_selection)
                as Box<dyn FnMut(&mut VisualTestAppContext)>,
        ),
        (
            "inventory_tool_matrix",
            Box::new(contracts::scenario_inventory_matrix),
        ),
        (
            "inventory_tool_existing_runner",
            Box::new(contracts::scenario_inventory_existing_runner),
        ),
    ]);
}

pub(crate) fn prepare(filters: Option<&[String]>, exact: Option<&str>) -> Result<bool, String> {
    let requested = filters.is_some_and(|filters| {
        filters
            .iter()
            .any(|filter| filter.starts_with("inventory:"))
    }) || exact.is_some_and(|name| name.starts_with("inventory:"));
    if requested
        && filters.is_some_and(|filters| {
            filters
                .iter()
                .any(|filter| !filter.starts_with("inventory:"))
        })
    {
        return Err("inventory: selections cannot be mixed with ordinary scenarios".into());
    }
    if requested {
        capture::prepare(exact.is_none())?;
    }
    Ok(requested)
}

/// Preserve the supervisor's final failure even when the child could not unwind.
pub(crate) fn record_child_failure(name: &str, reason: &str) {
    if let Some(entry) = table::INVENTORY.iter().find(|entry| entry.name == name) {
        capture::record_child_failure(entry, reason);
    }
}
