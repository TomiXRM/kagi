use super::*;

// ── fuzzy matcher ──────────────────────────────────────────────────

#[test]
fn fuzzy_empty_query_matches_anything() {
    assert_eq!(fuzzy_match("", "anything"), Some(0));
}

#[test]
fn fuzzy_subsequence_matches_and_is_case_insensitive() {
    // "nb" is a subsequence of "New Branch…" (N…B).
    assert!(fuzzy_match("nb", "New Branch…").is_some());
    assert!(fuzzy_match("NB", "new branch…").is_some());
    // "push" is a contiguous substring.
    assert!(fuzzy_match("push", "Push").is_some());
}

#[test]
fn fuzzy_non_subsequence_returns_none() {
    // A char not present at all → None.
    assert_eq!(fuzzy_match("zzz", "New Branch"), None);
    // Order matters: 'h' only appears at the very end, so no 'b' follows it.
    assert_eq!(fuzzy_match("hb", "New Branch"), None);
}

#[test]
fn fuzzy_word_start_beats_scattered() {
    // Both contain the letters, but a word-start / contiguous hit must
    // outrank a scattered one. This is what makes ranking meaningful.
    let contiguous = fuzzy_match("branch", "New Branch").unwrap();
    // Same letters, but interleaved with 'x' so none are contiguous and only
    // the first sits at a word start.
    let scattered = fuzzy_match("branch", "bxrxaxnxcxh").unwrap();
    assert!(
        contiguous > scattered,
        "contiguous {contiguous} should beat scattered {scattered}"
    );
}

// ── row-model builder ──────────────────────────────────────────────

fn all_enabled(_id: &'static str) -> CommandState {
    CommandState::Enabled
}
fn no_keys(_id: &'static str) -> Option<String> {
    None
}

#[test]
fn build_rows_empty_query_enumerates_all_commands() {
    let rows = build_rows("", |id| id.to_string(), all_enabled, no_keys);
    // Static `theme.*` / `lang.*` commands are replaced by dynamic rows
    // (issue #373), so the count is the non-dynamic commands plus one row
    // per theme and per language.
    let static_cmds = COMMANDS
        .iter()
        .filter(|c| !is_dynamic_command(c.id) && !c.id.starts_with("repo.conflict"))
        .count();
    assert_eq!(
        rows.len(),
        static_cmds + theme::all_themes().len() + Lang::ALL.len()
    );
    // Registry order is preserved for the static-command prefix.
    for (row, cmd) in rows.iter().zip(
        COMMANDS
            .iter()
            .filter(|c| !is_dynamic_command(c.id) && !c.id.starts_with("repo.conflict")),
    ) {
        assert_eq!(row.id, cmd.id);
    }
    assert!(!rows.iter().any(|row| row.id.starts_with("repo.conflict")));
}

#[test]
fn build_rows_includes_every_theme_and_language() {
    // Issue #373: the palette must offer EVERY registered theme and EVERY
    // UI language, not just the handful with a static `theme.*` /
    // `lang.*` command. Fails before the dynamic-append fix (only 6 of
    // the themes had a static command).
    let rows = build_rows("", |id| id.to_string(), all_enabled, no_keys);
    let themes = theme::all_themes();
    for t in &themes {
        let slug = SharedString::from(t.slug.clone());
        assert!(
            rows.iter()
                .any(|r| r.action == PaletteAction::SetTheme(slug.clone())),
            "palette missing theme {}",
            t.slug
        );
    }
    for &l in Lang::ALL {
        assert!(
            rows.iter().any(|r| r.action == PaletteAction::SetLang(l)),
            "palette missing language {}",
            l.slug()
        );
    }
    // Every theme slug is present exactly once (no duplicate static row).
    let theme_rows = rows
        .iter()
        .filter(|r| matches!(r.action, PaletteAction::SetTheme(_)))
        .count();
    assert_eq!(theme_rows, themes.len());
}

#[test]
fn build_rows_fuzzy_filters_to_subsequence_matches() {
    // Label the commands by their own English registry label so we search
    // real labels. "push" should surface repo.push and only matching rows.
    let rows = build_rows(
        "push",
        |id| command_label(id, Lang::En).to_string(),
        all_enabled,
        no_keys,
    );
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| fuzzy_match("push", &r.label).is_some()));
    assert!(rows.iter().any(|r| r.id == "repo.push"));
    // A command with no 'push' subsequence is excluded.
    assert!(rows.iter().all(|r| r.id != "repo.fetch"));
}

#[test]
fn build_rows_disabled_command_carries_command_state_reason() {
    let state_of = |id: &'static str| {
        if id == "repo.push" {
            CommandState::Disabled("no repo open")
        } else {
            CommandState::Enabled
        }
    };
    let rows = build_rows("", |id| id.to_string(), state_of, no_keys);
    let push = rows.iter().find(|r| r.id == "repo.push").unwrap();
    assert!(!push.enabled);
    assert_eq!(push.disabled_reason.as_deref(), Some("no repo open"));
    // An enabled row has no reason.
    let other = rows.iter().find(|r| r.id == "repo.fetch").unwrap();
    assert!(other.enabled);
    assert_eq!(other.disabled_reason, None);
}

#[test]
fn build_rows_keystroke_is_carried_for_the_right_edge() {
    let keys = |id: &'static str| {
        if id == "repo.push" {
            Some("Cmd+Shift+K".to_string())
        } else {
            None
        }
    };
    let rows = build_rows("", |id| id.to_string(), all_enabled, keys);
    let push = rows.iter().find(|r| r.id == "repo.push").unwrap();
    assert_eq!(push.keystroke.as_deref(), Some("Cmd+Shift+K"));
}

#[test]
fn selected_command_id_resolves_from_row() {
    // "test: selecting a command runs it" — the run path is
    // `rows[selected].id → handle_menu_command`. Assert the row model
    // carries the id that the click/Enter handlers dispatch.
    let rows = build_rows(
        "settings",
        |id| command_label(id, Lang::En).to_string(),
        all_enabled,
        no_keys,
    );
    assert_eq!(rows.first().map(|r| &*r.id), Some("app.settings"));
}

// ── i18n: EN + JA both render ──────────────────────────────────────

#[test]
fn command_label_renders_en_and_ja() {
    // A localizable label differs between languages…
    let en = command_label("app.settings", Lang::En);
    let ja = command_label("app.settings", Lang::Ja);
    assert_eq!(en, "Settings…");
    assert_eq!(ja, "設定…");
    assert_ne!(en, ja);
    // …while a domain word stays English in both (ADR-0048).
    assert_eq!(command_label("repo.push", Lang::En), "Push");
    assert_eq!(command_label("repo.push", Lang::Ja), "Push");
}

#[test]
fn every_command_has_a_ja_label_or_is_an_english_domain_word() {
    // Guard: no command renders as its raw id in Japanese (that would be a
    // missing translation, not a deliberate domain word).
    for cmd in COMMANDS {
        let ja = command_label(cmd.id, Lang::Ja);
        assert_ne!(ja, cmd.id, "command {} has no JA label", cmd.id);
    }
}
