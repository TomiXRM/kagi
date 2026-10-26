- Deliverable / evidence: Created only [examples/lab_i05_s1.rs](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/examples/lab_i05_s1.rs), gallery page `lab-i05-s1`.
  
  Commands run: `rustfmt --edition 2021 examples/lab_i05_s1.rs`, file-only `rustfmt --check`, `cargo build --example lab_i05_s1`, `cargo run --example lab_i05_s1`, binary `--list`, all-state capture runs, and unknown-state validation (exit 2, valid states listed).
  
  All 18 final PNGs were opened and inspected in [the evidence directory](/Users/tomixrm/.t3/worktrees/git-client/experiment-kagi-settings-skill/target/ui-lab/runs/i05/s1):
  
  `lab-i05-s1-{state}-{theme}-en.png`, covering all seven listed states × `apple-dark`/`apple-light`.
  
  `lab-i05-s1-default-{theme}-ja-720x{height}.png`, covering both themes × heights `692`/`820`.
  
  Captures used `KAGI_GALLERY_DISPLAY=ad1ab626` without activation.

- Requirement self-check:  
  1. ○ Apple Dark and Light EN host renders succeeded.  
  2. ○ All existing control kinds use the kit.  
  3. ○ Required categories, order, text, status rows and switch values match.  
  4. ○ Reusable picker takes explicit selection and forced state; rest, selected and focus states are listed.  
  5. ○ Kit row geometry retained; trailing controls align. All rows appear at the default 900×820 size.  
  6. ○ Seven states captured; focus, pressed and hover states visibly differ while preserving values.  
  7. ○ Commands, evidence and observation limits reported.
  
  Critique: indicators remain inside their tracks/cells; sibling controls look consistent; sidebar tiles all use Accent; both themes remain readable. **[C1] Fixed** faint picker-check contrast using `ink::card()`. **[C2] None unresolved.**

- Unclear points: [Execution] Capture refused as “not visible” / host reported `occlusionState 8192` / retry later on the specified display without activation. [Planning] Last row falls below the 720×692 viewport / kit heights exceed available space / capture at increased height; 720×820 shows everything.

- Discretionary fills: Default size 900×820; nearest available icons; compact picker geometry. Reference wording stays English under the `ja` environment.

- Kit gaps: Page-local `position_picker(selected, forced)` only, using theme tokens and kit sizing/ring helpers.

- Retries: Three runtime capture retry rounds: two visibility refusals and one incidental-hover capture. One visual correction followed by recapture. Cargo locks were waited out.

- Not observed: Controlled live hover, real keyboard focus, VoiceOver, live picker selection, persistent toggling/navigation, or scrolling to the final row at 720×692. Tests and Clippy were not run, as requested.