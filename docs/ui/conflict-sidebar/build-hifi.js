// High-fidelity conflict workspace boards, revision 2 (after FableVisualReviewHifi).
// Reference: Apple's Xcode merge sheet (borderless side-by-side files, gutter
// "? ⌄" pull-down pill at each conflict, continuous red band, jump bar +
// commit footer per pane). Sidebar: Finder/Mail source list, top-down sections,
// selection fill only (no focus ring). Chrome: Kagi's unified toolbar with its
// right icon group. Tokens: crates/kagi-ui-core/src/theme_apple_{light,dark}.rs.
// Icons are drawn as vectors (SF-like strokes); UI text is Inter with Noto Sans
// JP runs for CJK (Kagi ships Inter + JetBrains Mono).
const page = figma.currentPage;
for (const child of [...page.children]) child.remove();

const LIGHT = {
  dark: false, base: '#ffffff', rowAlt: '#f5f5f5', surface: '#e5e5e5', selected: '#d9edff', inactiveSelected: '#d3d3d3',
  panel: '#f7f7f7', sidebar: '#f7f7f7', chrome: '#f7f7f7', text: '#000000', sub: '#606060', muted: '#666666',
  branch: '#0088ff', remote: '#34c759', success: '#008932', warning: '#c55300', modified: '#a16a00', blocker: '#e9152d',
  removedBg: '#ffe7e8', hairline: '#d9d9d9', elevated: '#ffffff', elevatedBorder: '#d9d9d9', shadow: 0.18,
  systemBlue: '#007aff', gutterBg: '#fafafa', termBg: '#ffffff', termCursor: '#000000',
  syn: { kw: '#9b2393', str: '#c41a16', num: '#1c00cf', cmt: '#5d6c79', ident: '#000000' }
};
const DARK = {
  dark: true, base: '#101012', rowAlt: '#18181a', surface: '#2c2c2e', selected: '#3a3a3c', inactiveSelected: '#3a3a3c',
  panel: '#161618', sidebar: '#101012', chrome: '#1c1c1e', text: '#ffffff', sub: '#98989f', muted: '#98989f',
  branch: '#ffd600', remote: '#30d158', success: '#30d158', warning: '#ff9230', modified: '#ffd600', blocker: '#ff4245',
  removedBg: '#402224', hairline: '#2a2a2c', elevated: '#2a2a2c', elevatedBorder: '#414144', shadow: 0.5,
  systemBlue: '#0a84ff', gutterBg: '#141416', termBg: '#1c1c1e', termCursor: '#ffd600',
  syn: { kw: '#fc5fa3', str: '#fc6a5d', num: '#d0bf69', cmt: '#6c7986', ident: '#ffffff' }
};

function hex(c) { const n = parseInt(c.slice(1), 16); return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255 }; }
function fill(c, opacity) { const p = { type: 'SOLID', color: hex(c) }; if (opacity !== undefined) p.opacity = opacity; return [p]; }
function rect(parent, name, x, y, w, h, color, o = {}) {
  const r = figma.createRectangle(); r.name = name; r.x = x; r.y = y; r.resize(Math.max(0.5, w), Math.max(0.5, h));
  r.fills = color ? fill(color, o.opacity) : [];
  if (o.radius) r.cornerRadius = o.radius;
  if (o.stroke) { r.strokes = fill(o.stroke); r.strokeWeight = o.strokeWeight || 1; }
  if (o.rotation) r.rotation = o.rotation;
  if (o.shadow) r.effects = [{ type: 'DROP_SHADOW', color: { r: 0, g: 0, b: 0, a: o.shadow }, offset: { x: 0, y: o.shadowY ?? 4 }, radius: o.shadowBlur ?? 12, spread: 0, visible: true, blendMode: 'NORMAL' }];
  parent.appendChild(r); return r;
}
function ellipse(parent, name, x, y, w, h, color) { const e = figma.createEllipse(); e.name = name; e.x = x; e.y = y; e.resize(w, h); e.fills = fill(color); parent.appendChild(e); return e; }
const CJK = /[\u3000-\u9FFF\uFF00-\uFFEF]/;
function textWidth(v, size) { let w = 0; for (const ch of v) w += CJK.test(ch) ? size : size * 0.56; return w; }
function text(parent, name, x, y, value, size, color, o = {}) {
  const t = figma.createText(); t.name = name; t.x = x; t.y = y;
  const style = o.bold ? 'Bold' : o.medium ? 'Medium' : 'Regular';
  t.fontName = { family: o.mono ? 'JetBrains Mono' : 'Inter', style: o.mono ? 'Regular' : style };
  t.characters = value; t.fontSize = size; t.fills = fill(color, o.opacity);
  if (!o.mono) { let i = 0; while (i < value.length) { let j = i; const cjk = CJK.test(value[i]); while (j < value.length && CJK.test(value[j]) === cjk) j++; if (cjk) t.setRangeFontName(i, j, { family: 'Noto Sans JP', style }); i = j; } }
  parent.appendChild(t); return t;
}
function frame(parent, name, x, y, w, h, color) { const f = figma.createFrame(); f.name = name; f.x = x; f.y = y; f.resize(w, h); f.fills = color ? fill(color) : []; f.clipsContent = true; parent.appendChild(f); return f; }
function middle(v, max, size) { if (textWidth(v, size) <= max) return v; const c = [...v]; let h = Math.floor(c.length / 2), t = c.length - h; while (h + t > 1 && textWidth(c.slice(0, h).join('') + '…' + c.slice(c.length - t).join(''), size) > max) { if (h >= t) h--; else t--; } return c.slice(0, h).join('') + '…' + c.slice(c.length - t).join(''); }

// ── Vector icons: SF-like stroked paths in a 20-unit box, scaled to `size` ──
const ICON_PATHS = {
  'chevron.right': ['M 7 4 L 13 10 L 7 16'],
  'chevron.left': ['M 13 4 L 7 10 L 13 16'],
  'chevron.down': ['M 4 7 L 10 13 L 16 7'],
  'arrow.down': ['M 10 3 L 10 17', 'M 4 11 L 10 17 L 16 11'],
  'arrow.up': ['M 10 17 L 10 3', 'M 4 9 L 10 3 L 16 9'],
  'plus': ['M 10 3 L 10 17', 'M 3 10 L 17 10'],
  'tray.and.arrow.down': ['M 10 2 L 10 11', 'M 6 7.5 L 10 11.5 L 14 7.5', 'M 3 12 L 3 17 L 17 17 L 17 12', 'M 3 12 L 7 12 L 8 14 L 12 14 L 13 12 L 17 12'],
  'folder': ['M 2.5 5 L 2.5 16 L 17.5 16 L 17.5 7 L 9.5 7 L 8 5 Z'],
  'arrow.uturn.backward': ['M 4 8 L 12.5 8 C 18 8 18 16 12.5 16 L 8 16', 'M 7 4.5 L 3.5 8 L 7 11.5'],
  'arrow.uturn.forward': ['M 16 8 L 7.5 8 C 2 8 2 16 7.5 16 L 12 16', 'M 13 4.5 L 16.5 8 L 13 11.5'],
  'terminal': ['M 2.5 4 L 17.5 4 L 17.5 16 L 2.5 16 Z', 'M 6 8 L 9 10.5 L 6 13', 'M 10.5 13 L 14 13'],
  'sidebar.left': ['M 2.5 4.5 L 17.5 4.5 L 17.5 15.5 L 2.5 15.5 Z', 'M 7.5 4.5 L 7.5 15.5'],
  'arrow.triangle.2.circlepath': ['M 15.5 7 C 14 4 7 2.5 4 8.5', 'M 4.5 13 C 6 16 13 17.5 16 11.5', 'M 2.5 5.5 L 4 8.8 L 7.2 7.3', 'M 17.5 14.5 L 16 11.2 L 12.8 12.7'],
  'lock.fill': ['M 6.5 9 L 6.5 6.5 C 6.5 2 13.5 2 13.5 6.5 L 13.5 9'],
  'point.3.connected': ['M 6 6 L 10 13', 'M 14 6 L 10 13'],
  'arrow.triangle.branch': ['M 6 4 L 6 16', 'M 6 11 C 6 8 10 8 13 6'],
  'square.and.pencil': ['M 11 4 L 4 4 L 4 16 L 16 16 L 16 9', 'M 9 12 L 16.5 4.5 L 15.5 3.5 L 8 11 L 7.5 12.5 Z'],
  'clock': ['M 10 10 L 10 5.5', 'M 10 10 L 13 12'],
  'gearshape': ['M 10 3 L 10 5', 'M 10 15 L 10 17', 'M 3 10 L 5 10', 'M 15 10 L 17 10', 'M 5 5 L 6.5 6.5', 'M 13.5 13.5 L 15 15', 'M 15 5 L 13.5 6.5', 'M 6.5 13.5 L 5 15'],
  'doc': ['M 5 2.5 L 11.5 2.5 L 15 6 L 15 17.5 L 5 17.5 Z', 'M 11.5 2.5 L 11.5 6 L 15 6', 'M 7.5 10.5 L 12.5 10.5', 'M 7.5 13.5 L 12.5 13.5']
};
const ICON_FILLS = {
  'folder.fill': ['M 2 5 C 2 4.2 2.6 3.6 3.4 3.6 L 7.6 3.6 L 9.2 5.4 L 16.6 5.4 C 17.4 5.4 18 6 18 6.8 L 18 15 C 18 15.8 17.4 16.4 16.6 16.4 L 3.4 16.4 C 2.6 16.4 2 15.8 2 15 Z'],
  'lock.fill': ['M 4.5 9 L 15.5 9 L 15.5 17 L 4.5 17 Z'],
  'point.3.connected': [],
  'arrow.triangle.branch': []
};
function pathMin(paths) {
  let mx = Infinity, my = Infinity;
  for (const d of paths) {
    const tokens = d.match(/[A-Za-z]|-?\d+(\.\d+)?/g) || [];
    let i = 0;
    while (i < tokens.length) {
      const cmd = tokens[i++];
      const nums = []; while (i < tokens.length && !/[A-Za-z]/.test(tokens[i])) nums.push(parseFloat(tokens[i++]));
      const pts = [];
      if (cmd === 'A') { for (let j = 0; j + 6 < nums.length; j += 7) pts.push([nums[j + 5], nums[j + 6]]); }
      else if (cmd !== 'Z') { for (let j = 0; j + 1 < nums.length; j += 2) pts.push([nums[j], nums[j + 1]]); }
      for (const [x, y] of pts) { mx = Math.min(mx, x); my = Math.min(my, y); }
    }
  }
  return [mx === Infinity ? 0 : mx, my === Infinity ? 0 : my];
}
function icon(parent, name, kind, x, y, size, color) {
  const g = frame(parent, `Icon/${name}`, x, y, size, size, null);
  const k = size / 20;
  const scalePath = (d) => d.replace(/-?\d+(\.\d+)?/g, (m) => String(Math.round(parseFloat(m) * k * 100) / 100));
  const strokes = ICON_PATHS[kind] || [];
  const fills = ICON_FILLS[kind] || [];
  const weight = Math.max(1.25, size / 14);
  const dot=(dx,dy,r)=>{const e=figma.createEllipse(); e.name='dot'; e.x=(dx-r)*k; e.y=(dy-r)*k; e.resize(r*2*k,r*2*k); e.fills=fill(color); g.appendChild(e);};
  if (kind === 'point.3.connected') { dot(6,6,2.5); dot(14,6,2.5); dot(10,13,2.5); }
  if (kind === 'arrow.triangle.branch') { dot(6,4,2.25); dot(6,16,2.25); dot(14,6,2.25); }
  if (kind === 'clock' || kind === 'gearshape') {
    const inset = (kind === 'clock' ? 2 : 5) * k;
    const ring = figma.createEllipse(); ring.name = 'ring'; ring.x = inset; ring.y = inset; ring.resize(size - inset * 2, size - inset * 2);
    ring.fills = []; ring.strokes = fill(color); ring.strokeWeight = weight; g.appendChild(ring);
  }
  const place = (v, paths) => { const [mx, my] = pathMin(paths); v.x = mx * k; v.y = my * k; };
  if (fills.length) { const v = figma.createVector(); v.name = 'fill'; v.vectorPaths = fills.map(d => ({ windingRule: 'NONZERO', data: scalePath(d) })); v.fills = fill(color); v.strokes = []; g.appendChild(v); place(v, fills); }
  if (strokes.length) { const v = figma.createVector(); v.name = 'stroke'; v.vectorPaths = strokes.map(d => ({ windingRule: 'NONZERO', data: scalePath(d) })); v.fills = []; v.strokes = fill(color); v.strokeWeight = weight; v.strokeCap = 'ROUND'; v.strokeJoin = 'ROUND'; g.appendChild(v); place(v, strokes); }
  return g;
}

// macOS push button (S): 20 px, radius 5.
function button(parent, name, x, y, label, T, kind, o = {}) {
  const w = o.w ?? Math.round(textWidth(label, 12) + 22);
  const g = frame(parent, `Button/${name}`, x, y, w, 20, null);
  const primary = kind === 'primary';
  rect(g, 'bg', 0, 0, w, 20, primary ? T.systemBlue : (T.dark ? '#3a3a3c' : '#ffffff'), { radius: 5, stroke: primary ? undefined : T.hairline });
  const ink = primary ? '#ffffff' : kind === 'unavailable' ? T.muted : T.text;
  text(g, 'label', (w - textWidth(label, 12)) / 2, 2, label, 12, ink, { medium: primary });
  return g;
}

// ── Kagi chrome: tab strip + unified toolbar with the right icon group ──
function kagiChrome(board, T, cfg) {
  const tabs = frame(board, 'TabStrip', 0, 0, cfg.width, 38, T.chrome);
  rect(tabs, 'hairline', 0, 37, cfg.width, 1, T.hairline);
  for (const [i, c] of ['#ff5f57', '#febc2e', '#28c840'].entries()) ellipse(tabs, 'traffic', 14 + i * 20, 13, 12, 12, c);
  rect(tabs, 'tab-active', 84, 0, 150, 38, T.base);
  text(tabs, 'tab-title', 96, 10, cfg.repo, 13, T.text, { medium: true });
  text(tabs, 'close', 212, 10, '×', 13, T.sub); text(tabs, 'new-tab', 246, 8, '+', 15, T.sub);
  const tb = frame(board, 'Toolbar', 0, 38, cfg.width, 52, T.chrome);
  rect(tb, 'hairline', 0, 51, cfg.width, 1, T.hairline);
  icon(tb, 'sync', 'arrow.triangle.2.circlepath', 14, 16, 20, T.text);
  text(tb, 'repo', 44, 10, cfg.repo, 13, T.text, { bold: true });
  text(tb, 'branch', 44, 28, `${cfg.branch} → origin/${cfg.branch}`, 11, T.sub);
  // Kagi collapses the center group behind » at narrow widths; the mock keeps five icons.
  const center = cfg.width < 1000 ? ['arrow.down', 'arrow.up', 'plus', 'tray.and.arrow.down', 'terminal'] : ['arrow.down', 'arrow.up', 'plus', 'tray.and.arrow.down', 'folder', 'arrow.uturn.backward', 'arrow.uturn.forward', 'terminal'];
  let ix = cfg.width < 1000 ? 300 : Math.round(cfg.width / 2) - 170;
  for (const [i, k] of center.entries()) { icon(tb, `tool-${i}`, k, ix, 16, 20, T.text); ix += 42; }
  // Right icon group (graph · merge · edit · history · settings), active = merge.
  const right = ['point.3.connected', 'arrow.triangle.branch', 'square.and.pencil', 'clock', 'gearshape'];
  let rx = cfg.width - 16 - right.length * 36;
  for (const [i, k] of right.entries()) { if (i === 1) rect(tb, 'mode-active', rx - 4, 12, 28, 28, T.surface, { radius: 5 }); icon(tb, `mode-${i}`, k, rx, 16, 20, T.text); rx += 36; }
  // Operation strip left of the icon group: "● merge" + actions, one 20 px row.
  let ax = cfg.width - 16 - right.length * 36 - 16;
  for (const [name, label, kind] of [...cfg.operationActions].reverse()) { const w = Math.round(textWidth(label, 12) + 22); ax -= w; button(tb, name, ax, 16, label, T, kind, { w }); ax -= 8; }
  const ol = textWidth(cfg.operationLabel, 12);
  text(tb, 'op-label', ax - ol - 8, 19, cfg.operationLabel, 12, T.sub);
  ellipse(tb, 'op-dot', ax - ol - 22, 23, 8, 8, T.warning);
  return 90;
}

// ── Source list sidebar (Finder / Mail): top-down sections, selection fill only ──
function sidebar(board, T, cfg, top, height) {
  const W = 232;
  const sb = frame(board, 'Sidebar', 0, top, W, height, T.sidebar);
  rect(sb, 'hairline-right', W - 1, 0, 1, height, T.hairline);
  let y = 0;
  const section = (name, label, rows) => {
    text(sb, `Section/${name}`, 16, y + 8, label, 11, T.sub, { bold: true });
    const count = String(rows.filter(r => !r.folder).length);
    text(sb, `Section/${name}/count`, W - 16 - textWidth(count, 11), y + 8, count, 11, T.sub);
    const g = frame(sb, `Group/${name}`, 0, y + 28, W, Math.max(1, rows.length * 24), null);
    let ry = 0;
    for (const r of rows) {
      const row = frame(g, `Row/${r.label}`, 0, ry, W, 24, null);
      if (r.selected) rect(row, 'selection', 8, 1, W - 16, 22, cfg.sidebarFocused === false ? T.inactiveSelected : T.selected, { radius: 6 });
      const x = 16 + (r.depth ?? 0) * 16;
      if (r.folder) {
        icon(row, 'disclosure', r.expanded ? 'chevron.down' : 'chevron.right', x, 7, 10, T.sub);
        icon(row, 'folder', 'folder.fill', x + 14, 5, 14, T.systemBlue);
        text(row, 'name', x + 34, 4, middle(r.label, W - x - 60, 13), 13, T.text);
      } else {
        icon(row, 'doc', 'doc', x + 14, 5, 14, T.sub);
        text(row, 'name', x + 34, 4, r.label, 13, T.text);
        if (r.badge === 'conflict' || r.badge === 'unsaved') {
          rect(row, 'badge', W - 30, 5, 14, 14, r.badge === 'conflict' ? T.blocker : T.modified, { radius: 3 });
          text(row, 'badge-text', W - 26, 3, r.badge === 'conflict' ? 'C' : 'M', 10, '#ffffff', { bold: true });
        } else if (r.badge === 'review') {
          text(row, 'badge-review', W - 28, 3, '!', 13, T.warning, { bold: true });
        }
      }
      ry += 24;
    }
    y += 28 + rows.length * 24 + 16;
  };
  section('InProgress', cfg.L.inProgress, cfg.upper);
  section('Saved', cfg.L.saved, cfg.lower);
  return W;
}

// ── Conflict header: toggle, jump bar, file action (trailing margin 16, panel bg) ──
function conflictHeader(board, T, cfg, left, top) {
  const w = cfg.width - left;
  const h = frame(board, 'ConflictHeader', left, top, w, 36, T.panel);
  rect(h, 'hairline', 0, 35, w, 1, T.hairline);
  const tg = frame(h, 'Button/SidebarToggle', 12, 8, 22, 20, null);
  if (!cfg.sidebarHidden) rect(tg, 'bg', 0, 0, 22, 20, T.surface, { radius: 5 });
  icon(tg, 'sidebar', 'sidebar.left', 4, 4, 13, T.text);
  let x = 42;
  if (cfg.sidebarHidden) { const n = String(cfg.upper.filter(r => !r.folder).length); text(h, 'toggle-count', 36, 11, n, 11, T.systemBlue, { medium: true }); x = 52; }
  for (const [i, seg] of cfg.breadcrumb.entries()) {
    const last = i === cfg.breadcrumb.length - 1;
    if (i) { icon(h, `crumb-sep-${i}`, 'chevron.right', x, 13, 10, T.sub); x += 14; }
    icon(h, `crumb-icon-${i}`, last ? 'doc' : 'folder.fill', x, 12, 12, last ? T.sub : T.systemBlue); x += 16;
    text(h, `crumb-${i}`, x, 10, seg, 12, last ? T.text : T.sub, { medium: last });
    x += textWidth(seg, 12) + 10;
  }
  if (cfg.fileStatus) text(h, 'file-status', x + 6, 11, cfg.fileStatus, 11, cfg.fileStatusColor || T.sub);
  let rx = w - 16;
  for (const [name, label, kind] of [...cfg.fileActions].reverse()) { const bw = Math.round(textWidth(label, 12) + 22); rx -= bw; button(h, name, rx, 8, label, T, kind, { w: bw }); rx -= 8; }
  return 36;
}

// ── Side-by-side files (Xcode) ──
function codeLine(pane, T, n, src, y) {
  text(pane, 'ln', 12, y, String(n), 11, T.sub, { mono: true, opacity: 0.7 });
  let x = 50;
  const tokens = src.match(/\/\/.*$|'[^']*'|\b(export|const|import|return|var|let)\b|\b\d+(\.\d+)?\b|\S+|\s+/g) || [];
  for (const tk of tokens) {
    const color = /^\/\//.test(tk) ? T.syn.cmt : /^'/.test(tk) ? T.syn.str : /^(export|const|import|return|var|let)$/.test(tk) ? T.syn.kw : /^\d/.test(tk) ? T.syn.num : T.syn.ident;
    if (tk.trim()) text(pane, 'tk', x, y, tk, 11.5, color, { mono: true });
    x += tk.length * 6.95;
  }
}
function filePane(parent, T, name, x, y, w, h, meta, lines, band, nav) {
  const p = frame(parent, name, x, y, w, h, T.base);
  rect(p, 'jump-bg', 0, 0, w, 26, T.panel); rect(p, 'jump-hairline', 0, 26, w, 1, T.hairline);
  ellipse(p, 'jump-dot', 12, 9, 8, 8, meta.color);
  text(p, 'jump-title', 28, 6, middle(meta.title, w - 44, 11.5), 11.5, T.text, { medium: true });
  let ly = 34;
  for (const [n, s] of lines) {
    if (n >= band[0] && n <= band[1]) rect(p, 'conflict-band', 0, ly - 2, w, 18, T.removedBg);
    if (n === band[0]) rect(p, 'conflict-stripe', 0, ly - 2, 3, 18 * (band[1] - band[0] + 1), T.blocker);
    codeLine(p, T, n, s, ly); ly += 18;
  }
  rect(p, 'footer-hairline', 0, h - 26, w, 1, T.hairline); rect(p, 'footer-bg', 0, h - 25, w, 25, T.panel);
  text(p, 'footer-text', 12, h - 21, middle(meta.footer, w - (nav ? 110 : 24), 11), 11, T.sub);
  if (nav) {
    // ‹ k/n › at the trailing end of the left footer (Xcode).
    const nw = textWidth(nav, 11);
    icon(p, 'nav-prev', 'chevron.left', w - 12 - nw - 36, h - 19, 12, T.sub);
    text(p, 'nav', w - 12 - nw - 18, h - 21, nav, 11, T.sub);
    icon(p, 'nav-next', 'chevron.right', w - 12 - 12, h - 19, 12, T.sub);
  }
  return p;
}
function gutter(parent, T, x, y, w, h, band, cfg) {
  const g = frame(parent, 'Gutter', x, y, w, h, T.gutterBg);
  rect(g, 'l', 0, 0, 1, h, T.hairline); rect(g, 'r', w - 1, 0, 1, h, T.hairline);
  const by = 34 + (band[0] - band[2]) * 18 - 2;
  rect(g, 'band', 0, by, w, 18 * (band[1] - band[0] + 1), T.removedBg);
  // "? ⌄" pull-down pill (38×20), decided state keeps the chevron.
  const pill = frame(g, 'Button/Decide', 3, by - 1, 38, 20, null);
  rect(pill, 'bg', 0, 0, 38, 20, cfg.decided ? T.selected : T.elevated, { radius: 10, stroke: cfg.decided ? undefined : T.elevatedBorder });
  text(pill, 'label', 9, 2, cfg.decided ? cfg.decidedLabel : '?', 12, cfg.decided ? T.text : T.systemBlue, { medium: true });
  icon(pill, 'chev', 'chevron.down', 25, 5, 10, cfg.decided ? T.sub : T.systemBlue);
  rect(g, 'footer-hairline', 0, h - 26, w, 1, T.hairline); rect(g, 'footer-bg', 0, h - 25, w, 25, T.panel);
  return g;
}
function decisionMenu(board, T, x, y, L) {
  const items = [L.chooseLeft, L.chooseRight, null, L.chooseLeftRight, L.chooseRightLeft];
  const w = 196, h = 4 + 22 * 4 + 11 + 4;
  const m = frame(board, 'Menu/Decide', x, y, w, h, null);
  rect(m, 'bg', 0, 0, w, h, T.elevated, { radius: 6, stroke: T.elevatedBorder, shadow: T.shadow, shadowY: T.dark ? 6 : 4, shadowBlur: T.dark ? 16 : 12 });
  let iy = 4;
  for (const it of items) {
    if (it === null) { rect(m, 'sep', 10, iy + 5, w - 20, 1, T.hairline); iy += 11; continue; }
    text(m, `item/${it}`, 14, iy + 3, it, 13, T.text); iy += 22;
  }
  return m;
}
function resultPane(board, T, x, y, w, h, cfg) {
  const p = frame(board, 'Result', x, y, w, h, T.base);
  rect(p, 'top-hairline', 0, 0, w, 1, T.hairline); rect(p, 'head-bg', 0, 1, w, 26, T.panel); rect(p, 'head-hairline', 0, 27, w, 1, T.hairline);
  text(p, 'title', 12, 7, cfg.L.result, 11.5, T.text, { medium: true });
  text(p, 'status', 12 + textWidth(cfg.L.result, 11.5) + 10, 7, cfg.resultStatus, 11, cfg.resultStatusColor || T.sub);
  let ly = 36;
  for (const [n, s, side] of cfg.resultLines) { if (side) rect(p, 'origin', 0, ly - 2, 3, 18, side === 'left' ? T.branch : side === 'right' ? T.remote : T.warning); codeLine(p, T, n, s, ly); ly += 18; }
  if (cfg.caretLine !== undefined) { const i = cfg.resultLines.findIndex(l => l[0] === cfg.caretLine); if (i >= 0) rect(p, 'caret', 50 + 6.95 * (cfg.caretCol ?? 0), 36 + i * 18 - 1, 1.5, 16, T.text); }
  if (cfg.resultClipped) rect(p, 'scroller', w - 9, 34, 5, Math.round((h - 40) * 0.55), T.text, { radius: 2.5, opacity: 0.25 });
  return p;
}
function savedPane(board, T, x, y, w, h, cfg) {
  const p = frame(board, 'SavedFile', x, y, w, h, T.base);
  rect(p, 'jump-bg', 0, 0, w, 26, T.panel); rect(p, 'jump-hairline', 0, 26, w, 1, T.hairline);
  icon(p, 'lock', 'lock.fill', 11, 7, 12, T.sub);
  text(p, 'jump-title', 28, 6, cfg.savedTitle, 11.5, T.text, { medium: true });
  let ly = 34; for (const [n, s] of cfg.savedLines) { codeLine(p, T, n, s, ly); ly += 18; }
  return p;
}
function bottomPanel(board, T, cfg, y, h) {
  const t = frame(board, 'BottomPanel', 0, y, cfg.width, h, T.termBg);
  rect(t, 'hairline', 0, 0, cfg.width, 1, T.hairline);
  let x = 12;
  for (const [i, tab] of [cfg.L.oplog, cfg.L.terminal, cfg.L.activity].entries()) { const w = Math.round(textWidth(tab, 12)) + 14; if (i === 1) rect(t, 'tab-selected', x - 7, 7, w, 20, T.surface, { radius: 5 }); text(t, `tab/${tab}`, x, 9, tab, 12, T.text); x += w + 8; }
  text(t, 'prompt', 12, 40, 'tomixrm-m1@tomixrm-m1 pricing-redesign % ', 12, T.text, { mono: true });
  rect(t, 'cursor', 12 + 41 * 6.95, 40, 7, 15, T.termCursor);
}
function statusBar(board, T, cfg, y) {
  const s = frame(board, 'StatusBar', 0, y, cfg.width, 24, T.chrome);
  rect(s, 'hairline', 0, 0, cfg.width, 1, T.hairline);
  let x = 12;
  text(s, 'branch', x, 5, cfg.branch, 11, T.text); x += textWidth(cfg.branch, 11) + 10;
  ellipse(s, 'dirty', x, 9, 7, 7, T.blocker); x += 12;
  text(s, 'conflicts', x, 5, `!${cfg.conflictCount}`, 11, T.blocker); x += 26;
  text(s, 'sync', x, 5, `↑0 ↓0   → origin/${cfg.branch}   ${cfg.time}   Ready`, 11, T.text);
  icon(s, 'term', 'terminal', cfg.width - 12 - 30, 5, 13, T.sub); icon(s, 'menu', 'sidebar.left', cfg.width - 12 - 12, 5, 13, T.sub);
}

const JA = { inProgress: '作業中', saved: '保存済み', result: '結果', oplog: '操作ログ', terminal: 'ターミナル', activity: 'アクティビティ', chooseLeft: '左を使う', chooseRight: '右を使う', chooseLeftRight: '左、次に右を使う', chooseRightLeft: '右、次に左を使う' };
const ctx = (n) => [n, `// Checkout policy context ${String(n).padStart(3, '0')}`];
function fileLines(from, to, at, value) { const out = []; for (let n = from; n <= to; n++) out.push(n === at ? [n, value] : ctx(n)); return out; }

function makeBoard(name, x, y, T, cfg) {
  const board = frame(page, name, x, y, cfg.width, cfg.height, T.base);
  const top = kagiChrome(board, T, cfg);
  const bottomH = cfg.terminalH, statusH = 24;
  const workH = cfg.height - top - bottomH - statusH;
  let left = 0;
  if (!cfg.sidebarHidden) left = sidebar(board, T, cfg, top, workH);
  const headerH = conflictHeader(board, T, cfg, left, top);
  const cx = left, cy = top + headerH, cw = cfg.width - left, ch = workH - headerH;
  if (cfg.mode === 'saved') {
    savedPane(board, T, cx, cy, cw, ch, cfg);
  } else {
    const maxRows = Math.floor((Math.round(ch * 0.38) - 44) / 18);
    const resultRows = Math.min(cfg.resultLines.length, maxRows);
    const resultH = 28 + resultRows * 18 + 16;
    const panesH = ch - resultH;
    const gutterW = 44;
    const paneW = Math.floor((cw - gutterW) / 2);
    const rows = Math.floor((panesH - 34 - 26) / 18);
    const at = cfg.conflictLine;
    const from = Math.max(1, at - Math.floor(rows / 2) + 1);
    const to = from + rows - 1;
    filePane(board, T, 'Pane/Left', cx, cy, paneW, panesH, { color: T.branch, title: cfg.leftTitle, footer: cfg.leftFooter }, fileLines(from, to, at, cfg.leftValue), [at, at, from], cfg.nav);
    gutter(board, T, cx + paneW, cy, gutterW, panesH, [at, at, from], cfg);
    filePane(board, T, 'Pane/Right', cx + paneW + gutterW, cy, paneW, panesH, { color: T.remote, title: cfg.rightTitle, footer: cfg.rightFooter }, fileLines(from, to, at, cfg.rightValue), [at, at, from], null);
    if (cfg.menuOpen) decisionMenu(board, T, cx + paneW + 3, cy + 34 + (at - from) * 18 - 3 + 22, cfg.L);
    resultPane(board, T, cx, cy + panesH, cw, resultH, { ...cfg, resultLines: cfg.resultLines.slice(0, resultRows), resultClipped: resultRows < cfg.resultLines.length });
  }
  bottomPanel(board, T, cfg, top + workH, bottomH);
  statusBar(board, T, cfg, cfg.height - statusH);
  return board;
}

const base = {
  width: 1440, height: 900, terminalH: 118, repo: 'pricing-redesign', branch: 'pricing-redesign', L: JA, time: '19:44:44',
  operationLabel: 'merge', operationActions: [['abort', '中止', null], ['continue', '確認へ進む', 'secondary']],
  breadcrumb: ['src', 'pricing', 'checkout.ts'], fileStatus: 'conflict 1/3',
  fileActions: [['save', '保存', 'unavailable']],
  leftTitle: 'pricing-redesign — 現在の側', rightTitle: 'c3b74a7a Adjust tax and add pickup shipping — 取り込む側',
  leftFooter: 'pricing-redesign · ff39fddd · Adjust pricing and add express shipping', rightFooter: 'c3b74a7a · Adjust tax and add pickup shipping',
  conflictLine: 8, leftValue: 'export const taxRate = 0.08;', rightValue: 'export const taxRate = 0.10;', nav: '1/3', conflictCount: 3,
  resultStatus: '下書き · 1/3', resultLines: [[7, '// Checkout policy context 007'], [8, 'export const taxRate = 0.10;', 'right'], [9, '// Checkout policy context 009'], [35, 'export const shippingMethods = ['], [36, "  'standard',"], [37, "  'express',", 'left'], [38, "  'pickup',", 'right'], [39, '];']],
  caretLine: 38, caretCol: 12,
  upper: [
    { folder: true, label: 'src', expanded: true },
    { folder: true, label: 'pricing', depth: 1, expanded: true },
    { label: 'checkout.ts', depth: 2, badge: 'conflict', selected: true },
    { label: 'rules.conf', depth: 2, badge: 'unsaved' },
    { folder: true, label: 'docs', expanded: true },
    { label: 'long-policy.txt', depth: 1, badge: 'conflict' }
  ],
  lower: [
    { folder: true, label: 'src', expanded: true },
    { folder: true, label: 'pricing', depth: 1, expanded: true },
    { label: 'shipping.ts', depth: 2 }
  ]
};

makeBoard('H1 Decide · Apple Light · JA', 0, 0, LIGHT, { ...base, menuOpen: true, sidebarFocused: false });
makeBoard('H2 Decided · Apple Light · JA', 1540, 0, LIGHT, { ...base, decided: true, decidedLabel: '右', nav: '2/3', conflictLine: 37, leftValue: "  'express',", rightValue: "  'pickup',", fileStatus: '未保存', fileStatusColor: LIGHT.warning, fileActions: [['save', '保存', 'primary']], resultStatus: '下書き · 3/3', caretLine: undefined, sidebarFocused: false, upper: base.upper.map(r => r.label === 'checkout.ts' ? { ...r, badge: 'unsaved' } : r) });
makeBoard('H3 Saved · Apple Light · JA', 3080, 0, LIGHT, { ...base, mode: 'saved', fileStatus: '保存済み · ステージ済み', fileStatusColor: LIGHT.success, fileActions: [['next-file', '次の作業中ファイル', 'secondary']], conflictCount: 2,
  savedTitle: 'ステージ済みの内容 — checkout.ts', savedLines: [[6, '// Checkout policy context 006'], [7, '// Checkout policy context 007'], [8, 'export const taxRate = 0.10;'], [9, '// Checkout policy context 009'], [10, '// Checkout policy context 010'], [35, 'export const shippingMethods = ['], [36, "  'standard',"], [37, "  'express',"], [38, "  'pickup',"], [39, '];'], [52, "export const supportNote = 'Contact support within 14 days.';"]],
  upper: [
    { folder: true, label: 'src', expanded: true }, { folder: true, label: 'pricing', depth: 1, expanded: true },
    { label: 'rules.conf', depth: 2, badge: 'unsaved' },
    { folder: true, label: 'docs', expanded: true }, { label: 'long-policy.txt', depth: 1, badge: 'conflict' }
  ],
  lower: [
    { folder: true, label: 'src', expanded: true }, { folder: true, label: 'pricing', depth: 1, expanded: true },
    { label: 'checkout.ts', depth: 2, selected: true }, { label: 'shipping.ts', depth: 2 }
  ] });
makeBoard('H4 All saved · Apple Light · JA', 4620, 0, LIGHT, { ...base, mode: 'saved', operationActions: [['abort', '中止', null], ['continue', 'コミットを確認', 'primary']], breadcrumb: ['src', 'pricing', 'rules.conf'], fileStatus: '保存済み · ステージ済み', fileStatusColor: LIGHT.success, fileActions: [], conflictCount: 0,
  savedTitle: 'ステージ済みの内容 — rules.conf', savedLines: [[1, 'notice = required'], [2, 'retention_days = 30']],
  upper: [],
  lower: [
    { folder: true, label: 'src', expanded: true }, { folder: true, label: 'pricing', depth: 1, expanded: true },
    { label: 'checkout.ts', depth: 2 }, { label: 'rules.conf', depth: 2, selected: true }, { label: 'shipping.ts', depth: 2 },
    { folder: true, label: 'docs', expanded: true }, { label: 'long-policy.txt', depth: 1 }
  ] });
makeBoard('H5 Hidden · Apple Light · JA', 6160, 0, LIGHT, { ...base, sidebarHidden: true });
makeBoard('H6 Narrow · Apple Dark · JA', 7700, 0, DARK, { ...base, width: 940, height: 660, terminalH: 96, menuOpen: true, sidebarFocused: false });
makeBoard('H7 Narrow hidden · Apple Dark · JA', 8740, 0, DARK, { ...base, width: 940, height: 660, terminalH: 96, sidebarHidden: true });

figma.currentPage.children.map(n => `${n.name} ${n.width}x${n.height}`);
