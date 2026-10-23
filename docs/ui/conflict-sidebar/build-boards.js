// Revision 2 of the conflict-sidebar boards: applies the Fable cross-review
// (FableReviewConflictSidebar). Controls stay S=24; no explanatory prose.
const page = figma.currentPage;
for (const child of [...page.children]) child.remove();

const LIGHT = {
  window: '#f5f5f7', toolbar: '#ececf0', sidebar: '#f0f0f4', surface: '#ffffff', border: '#d9d9de',
  text: '#1d1d1f', sub: '#6e6e73', accent: '#2f6fed', selectedText: '#ffffff', current: '#dbe7ff',
  incoming: '#dcf3e4', warning: '#9a4a00', danger: '#c0392b', success: '#1f7a3f', code: '#1d1d1f',
  gutter: '#8e8e93', terminal: '#ffffff', ring: '#2f6fed', muted: '#9a9aa0', wash: '#dedee3'
};
const DARK = {
  window: '#1e1e22', toolbar: '#2a2a30', sidebar: '#232328', surface: '#2b2b31', border: '#3c3c44',
  text: '#f2f2f5', sub: '#a6a6ae', accent: '#4c8dff', selectedText: '#ffffff', current: '#27385f',
  incoming: '#24442f', warning: '#f5a524', danger: '#ff6b5e', success: '#5ad17a', code: '#f2f2f5',
  gutter: '#86868e', terminal: '#121214', ring: '#4c8dff', muted: '#6c6c74', wash: '#3a3a42'
};

function hex(color) {
  const n = parseInt(color.slice(1), 16);
  return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255 };
}
function fill(color) { return [{ type: 'SOLID', color: hex(color) }]; }
function rect(parent, name, x, y, w, h, color, opts = {}) {
  const r = figma.createRectangle();
  r.name = name; r.x = x; r.y = y; r.resize(w, h);
  r.fills = color ? fill(color) : [];
  if (opts.radius) r.cornerRadius = opts.radius;
  if (opts.stroke) { r.strokes = fill(opts.stroke); r.strokeWeight = opts.strokeWeight || 1; }
  parent.appendChild(r);
  return r;
}
function textWidth(value, size) {
  let w = 0;
  for (const ch of value) w += /[\u3000-\u9FFF\uFF00-\uFFEF]/.test(ch) ? size : size * 0.58;
  return w;
}
function text(parent, name, x, y, value, size, color, opts = {}) {
  const t = figma.createText();
  t.name = name; t.x = x; t.y = y;
  const glyphOnly = /^[\u25A0-\u25FF\u2700-\u27BF\u2190-\u21FF\s]+$/.test(value);
  const family = opts.mono ? 'JetBrains Mono' : glyphOnly ? 'Noto Sans Symbols 2' : 'Noto Sans JP';
  const style = glyphOnly || opts.mono ? 'Regular' : opts.bold ? 'Bold' : (opts.medium ? 'Medium' : 'Regular');
  t.fontName = { family, style };
  t.characters = value; t.fontSize = size; t.fills = fill(color);
  parent.appendChild(t);
  return t;
}
function frame(parent, name, x, y, w, h, color) {
  const f = figma.createFrame();
  f.name = name; f.x = x; f.y = y; f.resize(w, h);
  f.fills = color ? fill(color) : [];
  f.clipsContent = true;
  parent.appendChild(f);
  return f;
}
function middleEllipsis(value, maxWidth, size) {
  if (textWidth(value, size) <= maxWidth) return value;
  const chars = [...value];
  let head = Math.floor(chars.length / 2), tail = chars.length - head;
  while (head + tail > 1 && textWidth(chars.slice(0, head).join('') + '…' + chars.slice(chars.length - tail).join(''), size) > maxWidth) {
    if (head >= tail) head -= 1; else tail -= 1;
  }
  return chars.slice(0, head).join('') + '…' + chars.slice(chars.length - tail).join('');
}
// kind: 'primary' | 'secondary' (outline) | 'unavailable' (outline, muted ink, reason in AX) | undefined (surface)
function button(parent, name, x, y, w, label, T, kind, opts = {}) {
  const bg = kind === 'primary' ? T.accent : T.surface;
  const fg = kind === 'primary' ? '#ffffff' : kind === 'unavailable' ? T.muted : T.text;
  const g = frame(parent, `Button/${name}`, x, y, w, 24, null);
  rect(g, 'bg', 0, 0, w, 24, bg, { radius: 6, stroke: kind === 'primary' ? undefined : T.border });
  if (opts.focus) rect(g, 'focus-ring', -2, -2, w + 4, 28, null, { radius: 8, stroke: T.ring, strokeWeight: 2 });
  const chevron = label.endsWith(' ▾');
  const caption = chevron ? label.slice(0, -2) : label;
  const captionW = textWidth(caption, 12.5);
  const startX = Math.max(4, (w - captionW - (chevron ? 14 : 0)) / 2);
  text(g, 'label', startX, 4, caption, 12.5, fg, { medium: true });
  if (chevron) text(g, 'chevron', startX + captionW + 4, 5, '▾', 11, fg);
  if (opts.aria) text(g, `aria/${opts.aria}`, 0, 30, '', 1, fg); // placeholder layer name only
  return g;
}
function icon(parent, name, x, y, glyph, color, size = 11) {
  return text(parent, `Icon/${name}`, x, y, glyph, size, color, { medium: true });
}
function chip(parent, name, rightX, y, label, T, role, onAccent) {
  const cw = Math.round(textWidth(label, 11)) + 12;
  const cx = rightX - cw;
  const ink = onAccent ? T.selectedText : role === 'warning' ? T.warning : role === 'danger' ? T.danger : T.text;
  rect(parent, `${name}/pill`, cx, y, cw, 16, onAccent ? null : T.surface, { radius: 8, stroke: onAccent ? T.selectedText : T.border });
  text(parent, `${name}/text`, cx + 6, y + 1, label, 11, ink, { medium: true });
  return cw;
}

const ROW = 20;
function treeRow(parent, opts) {
  const { y, depth, label, kind, chipLabel, chipRole, selected, focus, T, width, count, expanded } = opts;
  const g = frame(parent, `Row/${label}`, 0, y, width, ROW, null);
  if (selected) rect(g, 'selection', 6, 0, width - 12, ROW, T.accent, { radius: 5 });
  if (focus) rect(g, 'focus-ring', 6, 0, width - 12, ROW, null, { radius: 5, stroke: selected ? T.selectedText : T.ring, strokeWeight: 2 });
  const fg = selected ? T.selectedText : T.text;
  const sub = selected ? T.selectedText : T.sub;
  const x = 14 + depth * 14;
  if (kind === 'folder') {
    icon(g, 'disclosure', x, 4, expanded ? '▾' : '▸', sub);
    icon(g, 'folder', x + 12, 4, '▭', sub);
    const maxW = width - x - 28 - (expanded ? 14 : 44);
    text(g, 'name', x + 28, 3, middleEllipsis(label, maxW, 12.5), 12.5, fg, { medium: true });
    if (!expanded && count !== undefined) chip(g, 'count', width - 10, 2, String(count), T, 'neutral', selected);
  } else {
    icon(g, 'file', x + 12, 4, '▯', sub);
    text(g, 'name', x + 28, 3, label, 12.5, fg);
    if (chipLabel) chip(g, 'chip', width - 10, 2, chipLabel, T, chipRole, selected);
  }
  return g;
}

function group(sb, name, header, rows, y, h, T, opts = {}) {
  text(sb, `Header/${name}`, 12, y + 7, header, 11, T.sub, { bold: true });
  const g = frame(sb, `Group/${name}`, 0, y + 26, sb.width, h - 26, null);
  if (rows.length === 0) { text(g, 'empty', 26, 2, '—', 12.5, T.muted); return g; }
  let ry = 0;
  for (const row of rows) { treeRow(g, { ...row, y: ry, T, width: sb.width }); ry += ROW; }
  const contentH = rows.length * ROW;
  if (contentH > g.height) {
    // macOS overlay scroller thumb, drawn to make the clipped state explicit on the board.
    const thumbH = Math.max(24, Math.round(g.height * g.height / contentH));
    rect(g, 'scroller-thumb', sb.width - 8, 2, 4, thumbH, T.muted, { radius: 2 });
  }
  return g;
}

function sidebar(board, T, cfg, L) {
  const width = 248;
  const sb = frame(board, 'Sidebar', 0, 36, width, cfg.height - 36 - cfg.terminalH - 24, T.sidebar);
  rect(sb, 'divider-right', width - 1, 0, 1, sb.height, T.border);
  const upperFiles = cfg.upper.filter(r => r.kind === 'file');
  const lowerFiles = cfg.lower.filter(r => r.kind === 'file');
  const savedCount = lowerFiles.filter(r => r.chipRole !== 'warning').length;
  const EMPTY_H = 50;
  const total = sb.height;
  let upperH, lowerH;
  if (cfg.upper.length === 0) { upperH = EMPTY_H; lowerH = total - EMPTY_H; }
  else if (cfg.lower.length === 0) { lowerH = EMPTY_H; upperH = total - EMPTY_H; }
  else {
    // Lower group sized to content, capped at 40 %; the upper group receives the rest.
    const lowerContent = 26 + cfg.lower.length * ROW + 8;
    lowerH = Math.min(lowerContent, Math.round(total * 0.4));
    upperH = total - lowerH;
  }
  group(sb, 'InProgress', `${L.inProgress} (${upperFiles.length})`, cfg.upper, 0, upperH, T);
  rect(sb, 'divider-groups', 0, upperH, width, 1, T.border);
  const lowerHeader = cfg.lowerHeader ?? `${L.saved} (${savedCount})`;
  group(sb, 'Saved', lowerHeader, cfg.lower, upperH + 1, lowerH - 1, T);
  return sb;
}
function toolbar(board, T, cfg, L) {
  const tb = frame(board, 'Toolbar', 0, 0, cfg.width, 36, T.toolbar);
  rect(tb, 'divider', 0, 35, cfg.width, 1, T.border);
  const toggle = frame(tb, 'Button/SidebarToggle', 8, 6, 24, 24, null);
  // Shown: the same grey wash the active bottom tab uses. Hidden: plain surface + count badge.
  rect(toggle, 'bg', 0, 0, 24, 24, cfg.sidebarHidden ? T.surface : T.wash, { radius: 6, stroke: cfg.sidebarHidden ? T.border : undefined });
  if (cfg.toggleFocus) rect(toggle, 'focus-ring', -2, -2, 28, 28, null, { radius: 8, stroke: T.ring, strokeWeight: 2 });
  icon(toggle, 'sidebar', 6, 5, '▤', T.text);
  if (cfg.sidebarHidden) {
    const n = cfg.upper.filter(r => r.kind === 'file').length;
    rect(tb, 'toggle-badge', 24, 2, 16, 14, T.accent, { radius: 7 });
    text(tb, 'toggle-badge-text', 29, 2, String(n), 9.5, '#ffffff', { bold: true });
  }
  const opX = cfg.sidebarHidden ? 48 : 42;
  text(tb, 'operation', opX, 10, cfg.operation, 12.5, T.text, { medium: true });
  let px = opX + textWidth(cfg.operation, 12.5) + 10;
  if (cfg.operationChip) { px += chip(tb, 'operation-chip', px + Math.round(textWidth(cfg.operationChip, 11)) + 12, 10, cfg.operationChip, T, 'neutral', false) + 10; }
  text(tb, 'path', px, 10, cfg.path, 12.5, T.sub);
  let x = cfg.width - 8;
  for (const [name, label, kind, w] of [...cfg.actions].reverse()) {
    x -= w;
    button(tb, name, x, 6, w, label, T, kind);
    x -= 8;
  }
  return tb;
}

function codePane(parent, name, x, y, w, h, T, title, titleColor, lines, highlight) {
  const p = frame(parent, name, x, y, w, h, T.surface);
  rect(p, 'border', 0, 0, w, h, T.surface, { stroke: T.border });
  text(p, 'title', 10, 6, middleEllipsis(title, w - 20, 11.5), 11.5, titleColor, { medium: true });
  let ly = 28;
  for (const [n, s, hl] of lines) {
    if (hl) rect(p, 'highlight', 1, ly - 2, w - 2, 18, highlight);
    text(p, 'ln', 8, ly, String(n), 11, T.gutter, { mono: true });
    text(p, 'code', 40, ly, s, 11.5, T.code, { mono: true });
    ly += 18;
  }
  return p;
}

function contentHeader(c, T, cfg, L, w) {
  // Parent path in sub colour, basename in text colour, same 13 px.
  let x = 16;
  if (cfg.fileParent) {
    const parent = middleEllipsis(cfg.fileParent + '/', 360, 13);
    text(c, 'file-parent', x, 10, parent, 13, T.sub);
    x += textWidth(parent, 13) + 2;
  }
  text(c, 'file-title', x, 10, cfg.fileTitle, 13, T.text, { medium: true });
  x += textWidth(cfg.fileTitle, 13) + 12;
  if (cfg.leftAction) { const [n, label, kind, bw] = cfg.leftAction; button(c, n, x, 8, bw, label, T, kind); x += bw + 12; }
  text(c, 'file-status', x, 11, cfg.fileStatus, 11.5, cfg.fileStatusColor || T.sub, { medium: true });
  let bx = w - 16;
  for (const [name, label, kind, bw] of [...cfg.localActions].reverse()) {
    bx -= bw; button(c, name, bx, 8, bw, label, T, kind); bx -= 8;
  }
}

function resultLines(rp, cfg, T, startY = 10) {
  let ly = startY;
  for (const [n, s, side] of cfg.resultLines) {
    if (side) icon(rp, side, 10, ly + 1, side === 'current' ? '●' : side === 'manual' ? '✎' : '○', side === 'current' ? T.accent : side === 'manual' ? T.warning : T.success);
    text(rp, 'ln', 26, ly, String(n), 11, T.gutter, { mono: true });
    text(rp, 'code', 58, ly, s, 11.5, T.code, { mono: true });
    ly += 18;
  }
}

function contentHunk(board, T, cfg, L, left) {
  const w = cfg.width - left;
  const c = frame(board, 'Content', left, 36, w, cfg.height - 36 - cfg.terminalH - 24, T.window);
  contentHeader(c, T, cfg, L, w);
  const paneW = Math.floor((w - 48) / 2);
  // Whole file, scrolled so the current hunk is visible (Xcode / VS Code); no context toggle.
  const rows = cfg.width < 1000 ? 8 : 12;
  const paneH = 28 + rows * 18 + 8;
  const ctx = (n) => [n, `// Checkout policy context ${String(n).padStart(3, '0')}`];
  const around = (center, value) => {
    const out = [];
    for (let n = center - Math.floor(rows / 2) + 1; n <= center + Math.ceil(rows / 2); n++) out.push(n === center ? [n, value, true] : ctx(n));
    return out;
  };
  const cur = cfg.paneLines?.current ?? around(8, 'export const taxRate = 0.08;');
  const inc = cfg.paneLines?.incoming ?? around(8, 'export const taxRate = 0.10;');
  const curPane = codePane(c, 'Pane/Current', 16, 40, paneW, paneH, T, `● ${L.current} — pricing-redesign`, T.accent, cur, T.current);
  const incPane = codePane(c, 'Pane/Incoming', 32 + paneW, 40, paneW, paneH, T, `○ ${L.incoming} — c3b74a7a Adjust tax and add pickup shipping`, T.success, inc, T.incoming);
  // Overlay scroller thumbs: the panes are the whole file, not a snippet.
  for (const pane of [curPane, incPane]) rect(pane, 'scroller-thumb', paneW - 8, 30, 4, Math.round(paneH * 0.22), T.muted, { radius: 2 });
  let ax = 16;
  for (const [name, label, kind, bw] of cfg.adopt) { button(c, name, ax, 40 + paneH + 10, bw, label, T, kind); ax += bw + 8; }
  text(c, 'result-title', 16, 40 + paneH + 48, L.result, 13, T.text, { medium: true });
  const statusX = 16 + textWidth(L.result, 13) + 12;
  text(c, 'result-status', statusX, 40 + paneH + 49, cfg.resultStatus, 11.5, cfg.resultStatusColor || T.sub, { medium: true });
  button(c, 'edit', statusX + textWidth(cfg.resultStatus, 11.5) + 16, 40 + paneH + 44, 64, L.edit, T);
  const rpH = Math.min(c.height - (40 + paneH + 74) - 12, cfg.resultLines.length * 18 + 20);
  const rp = frame(c, 'Pane/Result', 16, 40 + paneH + 74, w - 32, rpH, T.surface);
  rect(rp, 'border', 0, 0, rp.width, rp.height, T.surface, { stroke: T.border });
  resultLines(rp, cfg, T);
  return c;
}

function contentReview(board, T, cfg, L, left) {
  const w = cfg.width - left;
  const c = frame(board, 'Content', left, 36, w, cfg.height - 36 - cfg.terminalH - 24, T.window);
  contentHeader(c, T, cfg, L, w);
  const top = 40;
  // Saved inspection sits on the sidebar surface (non-editable); drafts use the editor surface.
  const paneColor = cfg.inspection ? T.sidebar : T.surface;
  const rp = frame(c, cfg.inspection ? 'Pane/SavedResult' : 'Pane/Result', 16, top, w - 32, c.height - top - 12, paneColor);
  rect(rp, 'border', 0, 0, rp.width, rp.height, paneColor, { stroke: T.border });
  resultLines(rp, cfg, T);
  return c;
}

function terminal(board, T, cfg, L) {
  const y = cfg.height - cfg.terminalH - 24;
  const t = frame(board, 'BottomPanel', 0, y, cfg.width, cfg.terminalH, T.terminal);
  rect(t, 'divider', 0, 0, cfg.width, 1, T.border);
  let x = 12;
  for (const [i, tab] of [L.oplog, L.terminal, L.activity].entries()) {
    const w = Math.round(textWidth(tab, 12)) + 12;
    if (i === 1) rect(t, 'tab-selected', x - 6, 6, w, 22, T.wash, { radius: 5 });
    text(t, `tab/${tab}`, x, 9, tab, 12, T.text);
    x += w + 10;
  }
  text(t, 'prompt', 8, 40, 'tomixrm-m1@tomixrm-m1 pricing-redesign % ', 12, T.code, { mono: true });
}

function statusBar(board, T, cfg) {
  const s = frame(board, 'StatusBar', 0, cfg.height - 24, cfg.width, 24, T.toolbar);
  rect(s, 'divider', 0, 0, cfg.width, 1, T.border);
  text(s, 'branch', 10, 5, `pricing-redesign  ●  !${cfg.conflictCount}  ↑0 ↓0  → origin/pricing-redesign  Ready`, 11.5, T.text);
}

function makeBoard(name, x, y, T, cfg, L) {
  const board = frame(page, name, x, y, cfg.width, cfg.height, T.window);
  toolbar(board, T, cfg, L);
  let left = 0;
  if (!cfg.sidebarHidden) { sidebar(board, T, cfg, L); left = 248; }
  if (cfg.mode === 'review') contentReview(board, T, cfg, L, left); else contentHunk(board, T, cfg, L, left);
  terminal(board, T, cfg, L);
  statusBar(board, T, cfg);
  if (cfg.tooltip) {
    const tw = Math.round(textWidth(cfg.tooltip, 11.5)) + 16;
    const tip = frame(board, 'Tooltip/SidebarToggle', 8, 38, tw, 22, T.surface);
    rect(tip, 'bg', 0, 0, tw, 22, T.surface, { radius: 6, stroke: T.border });
    text(tip, 'text', 8, 3, cfg.tooltip, 11.5, T.text);
  }
  return board;
}

const JA = { inProgress: '作業中', saved: '保存済み', current: '現在の側', incoming: '取り込む側', result: '結果', edit: '編集', oplog: '操作ログ', terminal: 'ターミナル', activity: 'アクティビティ' };
const EN = { inProgress: 'In progress', saved: 'Saved', current: 'Current', incoming: 'Incoming', result: 'Result', edit: 'Edit', oplog: 'Operation Log', terminal: 'Terminal', activity: 'Activity' };

const resultPartial = [[7, '// Checkout policy context 007'], [8, 'export const taxRate = 0.10;', 'incoming'], [9, '// Checkout policy context 009'], [34, '// Checkout policy context 034'], [35, 'export const shippingMethods = [', null], [36, "  'standard',"], [37, "  'express',", 'current'], [38, "  'pickup',", 'incoming'], [39, '];']];
const resultReady = [[7, '// Checkout policy context 007'], [8, 'export const taxRate = 0.10;', 'incoming'], [9, '// Checkout policy context 009'], [35, 'export const shippingMethods = ['], [36, "  'standard',"], [37, "  'express',", 'current'], [38, "  'pickup',", 'incoming'], [39, '];'], [52, "export const supportNote = 'Contact support within 14 days.';", 'manual']];
const resultSaved = resultReady.map(([n, s]) => [n, s]);

// Toolbar: Continue is outline while any work remains; it is the sole primary only when nothing is in progress.
const jaActions = (continueLabel, continueKind) => [['abort', '中止', null, 56], ['more', 'その他', null, 56], ['continue', continueLabel, continueKind, 96]];
const enActions = (continueLabel, continueKind) => [['abort', 'Abort', null, 56], ['more', 'More', null, 56], ['continue', continueLabel, continueKind, 110]];

const base = {
  width: 1440, height: 900, terminalH: 120, operation: 'merge', path: 'src/pricing/checkout.ts',
  actions: jaActions('確認へ進む', 'secondary'),
  localActions: [['prev', '‹ 前へ', null, 64], ['next', '次へ ›', 'primary', 64], ['reset', 'すべてリセット', null, 96]],
  adopt: [['use-left', '左を使う', null, 88], ['use-right', '右を使う', null, 88], ['use-both', '両方を使う ▾', null, 112]],
  resultLines: resultPartial, conflictCount: 3,
  fileParent: 'src/pricing', fileTitle: 'checkout.ts', fileStatus: 'conflict 1/3', resultStatus: '下書き · 1/3', mode: 'hunk'
};

const upperMixed = [
  { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '1/3', chipRole: 'neutral', selected: true },
  { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning', focus: true },
  { kind: 'folder', depth: 0, label: 'docs', expanded: true },
  { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
];
const lowerOne = [
  { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'shipping.ts' }
];

makeBoard('01 Mixed state · JA · Light', 0, 0, LIGHT, { ...base, upper: upperMixed, lower: lowerOne }, JA);

makeBoard('02 Ready unsaved · JA · Light', 1540, 0, LIGHT, {
  ...base, mode: 'review', fileStatus: '未保存 · 保存できます', fileStatusColor: LIGHT.warning,
  actions: jaActions('確認へ進む', 'unavailable'),
  localActions: [['choices', '選択へ戻る', null, 88], ['save', 'この解決を保存', 'primary', 120]],
  upper: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '未保存', chipRole: 'warning', selected: true, focus: true },
    { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
  ],
  // External change after an earlier Save: the row stays in place but is no longer claimed as saved.
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'shipping.ts', chipLabel: '要確認', chipRole: 'warning' }
  ],
  resultLines: resultReady
}, JA);

makeBoard('03 Save accepted · JA · Light', 3080, 0, LIGHT, {
  ...base, mode: 'review', inspection: true, fileStatus: '保存済み · ステージ済み', fileStatusColor: LIGHT.success, conflictCount: 2,
  leftAction: ['next-file', '次の作業中ファイル', 'secondary', 128],
  localActions: [],
  upper: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
  ],
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts', selected: true, focus: true },
    { kind: 'file', depth: 1, label: 'shipping.ts' }
  ],
  resultLines: resultSaved
}, JA);

makeBoard('04 All saved · JA · Light', 4620, 0, LIGHT, {
  ...base, mode: 'review', inspection: true, fileStatus: '保存済み · ステージ済み', fileStatusColor: LIGHT.success, conflictCount: 0,
  operationChip: '未コミット',
  actions: jaActions('コミットを確認', 'primary'),
  localActions: [],
  upper: [],
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts' },
    { kind: 'file', depth: 1, label: 'rules.conf', selected: true },
    { kind: 'file', depth: 1, label: 'shipping.ts' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt' }
  ],
  fileTitle: 'rules.conf',
  resultLines: [[1, 'notice = required'], [2, 'retention_days = 30']]
}, JA);

makeBoard('05 Sidebar hidden · JA · Light', 6160, 0, LIGHT, { ...base, sidebarHidden: true, toggleFocus: true, tooltip: 'サイドバーを表示  ⌘⌥B', upper: upperMixed, lower: lowerOne }, JA);

const narrow = { ...base, width: 940, height: 660, terminalH: 110,
  actions: jaActions('確認へ進む', 'secondary'),
  localActions: [['prev', '‹', null, 28], ['next', '›', 'primary', 28], ['reset', 'リセット', null, 56]],
  adopt: [['use-left', '左を使う', null, 88], ['use-right', '右を使う', null, 88], ['use-both', '両方を使う ▾', null, 112]],
  resultLines: resultPartial.slice(0, 5) };
makeBoard('06a Narrow · JA · Dark · shown', 7700, 0, DARK, { ...narrow, upper: upperMixed, lower: lowerOne }, JA);
makeBoard('06b Narrow · JA · Dark · hidden', 8740, 0, DARK, { ...narrow, sidebarHidden: true, upper: upperMixed, lower: lowerOne }, JA);

makeBoard('07 Save refused · EN · Light', 0, 1000, LIGHT, {
  ...base, mode: 'review', fileStatus: 'Unsaved · save refused', fileStatusColor: LIGHT.danger,
  actions: enActions('Review commit', 'unavailable'),
  // Round-1 mock trials: a separate Reload read as "might discard the draft". Save itself performs the fresh
  // read inside plan → preflight, so the refusal state keeps one primary: save again. Detail stays in the Operation Log.
  localActions: [['log', 'Operation Log', null, 110], ['save', 'Save again', 'primary', 96]],
  lowerHeader: 'Saved (0)',
  upper: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: 'Unsaved', chipRole: 'warning', selected: true },
    { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: 'Unsaved', chipRole: 'warning' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
  ],
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'shipping.ts', chipLabel: 'Review needed', chipRole: 'warning' }
  ],
  resultLines: [[7, '// Checkout policy context 007'], [8, 'export const taxRate = 0.10;', 'incoming'], [37, "  'express',", 'current'], [38, "  'pickup',", 'incoming'], [52, "export const supportNote = 'Contact support within 14 days.';", 'manual']]
}, EN);

// ── Additional boards for the click-through prototype (round 2) ──
// After checkout.ts was saved: it sits in the lower group; rules.conf is the open upper file.
const upperRulesSelected = [
  { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning', selected: true, focus: true },
  { kind: 'folder', depth: 0, label: 'docs', expanded: true },
  { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
];
const lowerTwo = [
  { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'checkout.ts' },
  { kind: 'file', depth: 1, label: 'shipping.ts' }
];
makeBoard('09 rules.conf opened · JA · Light', 3080, 1000, LIGHT, {
  ...base, mode: 'review', fileParent: 'src/pricing', fileTitle: 'rules.conf', fileStatus: '未保存 · 保存できます', fileStatusColor: LIGHT.warning,
  path: 'src/pricing/rules.conf',
  localActions: [['choices', '選択へ戻る', null, 88], ['save', 'この解決を保存', 'primary', 120]],
  upper: upperRulesSelected, lower: lowerTwo, conflictCount: 2,
  resultLines: [[1, 'notice = required', 'incoming'], [2, 'retention_days = 30', 'current']]
}, JA);

const upperPolicySelected = [
  { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '1/3', chipRole: 'neutral' },
  { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning' },
  { kind: 'folder', depth: 0, label: 'docs', expanded: true },
  { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral', selected: true, focus: true }
];
makeBoard('10 long-policy opened · JA · Light', 4620, 1000, LIGHT, {
  ...base, fileParent: 'docs', fileTitle: 'long-policy.txt', fileStatus: 'conflict 1/1', resultStatus: '下書き · 0/1',
  path: 'docs/long-policy.txt',
  localActions: [['prev', '‹ 前へ', null, 64], ['next', '確認へ ›', 'primary', 72], ['reset', 'すべてリセット', null, 96]],
  upper: upperPolicySelected, lower: lowerOne,
  paneLines: { current: [[296, 'Returns are accepted within 30 days.'], [297, ''], [298, 'Refund window: 30 days', true], [299, 'Contact: support@example.com'], [300, '']], incoming: [[296, 'Returns are accepted within 30 days.'], [297, ''], [298, 'Refund window: 14 days', true], [299, 'Contact: support@example.com'], [300, '']] },
  resultLines: [[296, 'Returns are accepted within 30 days.'], [297, ''], [298, '—'], [299, 'Contact: support@example.com']]
}, JA);

makeBoard('11 Save refused · JA · Light', 6160, 1000, LIGHT, {
  ...base, mode: 'review', fileStatus: '未保存 · 保存を拒否', fileStatusColor: LIGHT.danger,
  actions: jaActions('確認へ進む', 'unavailable'),
  localActions: [['log', '操作ログ', null, 80], ['save', 'もう一度保存', 'primary', 110]],
  lowerHeader: '保存済み (0)',
  upper: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '未保存', chipRole: 'warning', selected: true },
    { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: '未保存', chipRole: 'warning' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt', chipLabel: '0/1', chipRole: 'neutral' }
  ],
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'shipping.ts', chipLabel: '要確認', chipRole: 'warning' }
  ],
  resultLines: resultReady
}, JA);

// Existing CommitPanel hand-off, drawn only so the prototype has a screen after コミットを確認.
makeBoard('12 Commit review · JA · Light', 7700, 1000, LIGHT, {
  ...base, mode: 'review', inspection: true, conflictCount: 0, operationChip: '未コミット',
  actions: [['abort', '中止', null, 56], ['more', 'その他', null, 56]],
  fileParent: '', fileTitle: 'マージコミット', fileStatus: '4 ファイルがステージ済み', fileStatusColor: LIGHT.success,
  localActions: [['back', '戻る', null, 56], ['commit', 'コミット', 'primary', 80]],
  upper: [],
  lower: [
    { kind: 'folder', depth: 0, label: 'src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'checkout.ts' },
    { kind: 'file', depth: 1, label: 'rules.conf' },
    { kind: 'file', depth: 1, label: 'shipping.ts' },
    { kind: 'folder', depth: 0, label: 'docs', expanded: true },
    { kind: 'file', depth: 1, label: 'long-policy.txt' }
  ],
  resultLines: [[1, "Merge branch 'origin/pricing-redesign' into pricing-redesign"], [2, ''], [3, '# Conflicts: checkout.ts, long-policy.txt, rules.conf, shipping.ts']]
}, JA);

const deepUpper = [
  { kind: 'folder', depth: 0, label: 'services/billing/src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '1/3', chipRole: 'neutral', selected: true },
  { kind: 'file', depth: 1, label: 'index.ts', chipLabel: '0/2', chipRole: 'neutral' },
  { kind: 'file', depth: 1, label: 'rules.conf', chipLabel: 'Unsaved', chipRole: 'warning' },
  { kind: 'folder', depth: 0, label: 'services/catalog/src/pricing', expanded: true },
  { kind: 'file', depth: 1, label: 'checkout.ts', chipLabel: '0/1', chipRole: 'neutral' },
  { kind: 'file', depth: 1, label: 'index.ts', chipLabel: '0/4', chipRole: 'neutral' },
  { kind: 'folder', depth: 0, label: 'docs/policies', expanded: false, count: 9 },
  { kind: 'folder', depth: 0, label: 'docs/regions', expanded: true },
  ...Array.from({ length: 14 }, (_, i) => ({ kind: 'file', depth: 1, label: `region-${String(i + 1).padStart(2, '0')}.md`, chipLabel: '0/1', chipRole: 'neutral' }))
];
makeBoard('08 Many deep paths · EN · Light', 1540, 1000, LIGHT, {
  ...base, upper: deepUpper, lower: [
    { kind: 'folder', depth: 0, label: 'services/billing/src/pricing', expanded: true },
    { kind: 'file', depth: 1, label: 'shipping.ts' }
  ],
  actions: enActions('Review commit', 'secondary'),
  localActions: [['prev', '‹ Prev', null, 64], ['next', 'Next ›', 'primary', 64], ['reset', 'Reset all', null, 80]],
  adopt: [['use-left', 'Use Left', null, 88], ['use-right', 'Use Right', null, 88], ['use-both', 'Use Both ▾', null, 104]],
  path: 'services/billing/src/pricing/checkout.ts', fileParent: 'services/billing/src/pricing', resultStatus: 'Draft · 1/3'
}, EN);

figma.currentPage.children.map(n => `${n.name} ${n.width}x${n.height}`);
