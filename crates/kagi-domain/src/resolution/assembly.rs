//! Borrowed traversal shared by assembly and Result comparison.
use super::{ConflictHunk, HunkChoice, HunkModel, LineOrder, LineOrigin, Region, ResolvedLine};

fn visit_group(
    lines: &[String],
    origin: LineOrigin,
    visit: &mut impl FnMut(&str, LineOrigin) -> bool,
) -> bool {
    lines.iter().all(|line| visit(line, origin))
}

fn visit_marker(
    standard: &str,
    width: usize,
    buffer: &mut String,
    visit: &mut impl FnMut(&str, LineOrigin) -> bool,
) -> bool {
    if width == 7 {
        return visit(standard, LineOrigin::Manual);
    }
    buffer.clear();
    buffer.extend(std::iter::repeat_n(standard.as_bytes()[0] as char, width));
    buffer.push_str(&standard[7..]);
    visit(buffer, LineOrigin::Manual)
}

impl ConflictHunk {
    pub(super) fn visit_resolved_lines(
        &self,
        visit: &mut impl FnMut(&str, LineOrigin) -> bool,
    ) -> bool {
        if let Some(selection) = &self.line_select {
            let current = (&self.current, &selection.current_taken, LineOrigin::Current);
            let incoming = (
                &self.incoming,
                &selection.incoming_taken,
                LineOrigin::Incoming,
            );
            let sides = match selection.order {
                LineOrder::CurrentFirst | LineOrder::CurrentBlockFirst => [current, incoming],
                LineOrder::IncomingFirst | LineOrder::IncomingBlockFirst => [incoming, current],
            };
            match selection.order {
                LineOrder::CurrentBlockFirst | LineOrder::IncomingBlockFirst => {
                    return sides.into_iter().all(|(lines, taken, origin)| {
                        lines
                            .iter()
                            .zip(taken)
                            .all(|(line, taken)| !taken || visit(line, origin))
                    });
                }
                LineOrder::CurrentFirst | LineOrder::IncomingFirst => {}
            }
            for index in 0..self.current.len().max(self.incoming.len()) {
                for (lines, taken, origin) in sides {
                    if taken.get(index) == Some(&true)
                        && lines.get(index).is_some_and(|line| !visit(line, origin))
                    {
                        return false;
                    }
                }
            }
            return true;
        }
        match &self.choice {
            HunkChoice::AcceptCurrent => visit_group(&self.current, LineOrigin::Current, visit),
            HunkChoice::AcceptIncoming => visit_group(&self.incoming, LineOrigin::Incoming, visit),
            HunkChoice::BothCurrentFirst => {
                visit_group(&self.current, LineOrigin::Current, visit)
                    && visit_group(&self.incoming, LineOrigin::Incoming, visit)
            }
            HunkChoice::BothIncomingFirst => {
                visit_group(&self.incoming, LineOrigin::Incoming, visit)
                    && visit_group(&self.current, LineOrigin::Current, visit)
            }
            HunkChoice::Manual(text) => text
                .split_terminator('\n')
                .all(|line| visit(line, LineOrigin::Manual)),
            HunkChoice::Unresolved => {
                // Reuse one marker buffer for wide materializations; the common
                // seven-character case visits static strings without allocation.
                let mut marker = if self.marker_width == 7 {
                    String::new()
                } else {
                    String::with_capacity(self.marker_width + 9)
                };
                visit_marker("<<<<<<< Current", self.marker_width, &mut marker, visit)
                    && visit_group(&self.current, LineOrigin::Manual, visit)
                    && (self.base.is_empty()
                        || (visit_marker("||||||| Base", self.marker_width, &mut marker, visit)
                            && visit_group(&self.base, LineOrigin::Manual, visit)))
                    && visit_marker("=======", self.marker_width, &mut marker, visit)
                    && visit_group(&self.incoming, LineOrigin::Manual, visit)
                    && visit_marker(">>>>>>> Incoming", self.marker_width, &mut marker, visit)
            }
        }
    }
}

impl HunkModel {
    pub(super) fn visit_resolved_lines(
        &self,
        visit: &mut impl FnMut(&str, LineOrigin) -> bool,
    ) -> bool {
        self.regions.iter().all(|region| match region {
            Region::Passthrough(lines) => visit_group(lines, LineOrigin::Context, visit),
            Region::Hunk(hunk) => hunk.visit_resolved_lines(visit),
        })
    }

    /// Compare Result text without allocating an assembled file or comparing
    /// provenance: manually typing the same bytes is not a divergent Result.
    pub fn matches_result_text(&self, result: &[ResolvedLine]) -> bool {
        let mut lines = result.iter();
        self.visit_resolved_lines(&mut |text, _| lines.next().is_some_and(|line| line.text == text))
            && lines.next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_assembly_preserves_empty_blank_and_crlf_text() {
        let mut model = HunkModel::from_marker_text(
            "<<<<<<< Current\ncurrent\n=======\nincoming\n>>>>>>> Incoming\n",
        );
        for (manual, expected) in [("", ""), ("\n", "\n"), ("a\r\n\nb", "a\r\n\nb\n")] {
            assert!(model.set_choice(0, HunkChoice::Manual(manual.into())));
            assert_eq!(model.assembled_text(), expected);
        }
    }
}
