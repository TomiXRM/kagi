//! Coupling graph layout for the Ecosystem "Graph" view — pure Rust, no deps
//! (ADR-0119).
//!
//! Turns the top change-coupling pairs into a node/edge graph (node = file,
//! edge = "these two change together") and lays it out with a deterministic
//! **Fruchterman–Reingold** force simulation: repulsion between every node pair,
//! attraction along edges, cooling over a fixed iteration count, seeded on a
//! circle (no RNG → identical layout every run). Output node positions are
//! normalized to the unit square `[0,1]²`, so the UI places nodes with
//! `relative()` lengths and paints edges on a `canvas` from the same coordinates.

use crate::hotspot::CouplingPair;
use std::collections::HashMap;
use std::f64::consts::TAU;

/// A file node. `x,y` are normalized to `[0,1]` after layout.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub file: String,
    /// Number of incident edges (its coupling fan-out).
    pub degree: u32,
    pub x: f64,
    pub y: f64,
}

/// An edge between `nodes[a]` and `nodes[b]`, weighted by co-change count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphEdge {
    pub a: usize,
    pub b: usize,
    pub weight: u32,
}

/// A laid-out coupling graph.
#[derive(Debug, Clone, PartialEq)]
pub struct CouplingGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Number of force-simulation iterations (fixed for determinism).
const ITERS: usize = 300;

/// Centre-gravity strength (linear in distance) — keeps disconnected components
/// from drifting far apart so the graph stays compact.
const GRAVITY: f64 = 0.1;

/// Build and lay out a graph from the top `max_edges` coupling pairs.
pub fn build_graph(pairs: &[CouplingPair], max_edges: usize) -> CouplingGraph {
    let pairs = &pairs[..pairs.len().min(max_edges)];
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    for p in pairs {
        let a = *index.entry(p.a.as_str()).or_insert_with(|| {
            nodes.push(GraphNode {
                file: p.a.clone(),
                degree: 0,
                x: 0.0,
                y: 0.0,
            });
            nodes.len() - 1
        });
        let b = *index.entry(p.b.as_str()).or_insert_with(|| {
            nodes.push(GraphNode {
                file: p.b.clone(),
                degree: 0,
                x: 0.0,
                y: 0.0,
            });
            nodes.len() - 1
        });
        nodes[a].degree += 1;
        nodes[b].degree += 1;
        edges.push(GraphEdge {
            a,
            b,
            weight: p.together,
        });
    }

    layout(&mut nodes, &edges);
    CouplingGraph { nodes, edges }
}

fn layout(nodes: &mut [GraphNode], edges: &[GraphEdge]) {
    let n = nodes.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        nodes[0].x = 0.5;
        nodes[0].y = 0.5;
        return;
    }

    // Seed on a circle (deterministic).
    for (i, node) in nodes.iter_mut().enumerate() {
        let ang = TAU * i as f64 / n as f64;
        node.x = 0.5 + 0.4 * ang.cos();
        node.y = 0.5 + 0.4 * ang.sin();
    }

    // Ideal edge length k; temperature cools linearly to 0.
    let k = (1.0 / n as f64).sqrt();
    let mut temp = 0.1_f64;
    let cool = temp / (ITERS as f64 + 1.0);
    let mut disp = vec![(0.0_f64, 0.0_f64); n];

    for _ in 0..ITERS {
        for d in disp.iter_mut() {
            *d = (0.0, 0.0);
        }
        // Repulsion between every pair: f = k²/d.
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = nodes[i].x - nodes[j].x;
                let dy = nodes[i].y - nodes[j].y;
                let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
                let f = k * k / dist;
                let (ux, uy) = (dx / dist, dy / dist);
                disp[i].0 += ux * f;
                disp[i].1 += uy * f;
                disp[j].0 -= ux * f;
                disp[j].1 -= uy * f;
            }
        }
        // Attraction along edges: f = d²/k.
        for e in edges {
            let dx = nodes[e.a].x - nodes[e.b].x;
            let dy = nodes[e.a].y - nodes[e.b].y;
            let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
            let f = dist * dist / k;
            let (ux, uy) = (dx / dist, dy / dist);
            disp[e.a].0 -= ux * f;
            disp[e.a].1 -= uy * f;
            disp[e.b].0 += ux * f;
            disp[e.b].1 += uy * f;
        }
        // Gravity toward the centre, proportional to distance — pulls drifting
        // disconnected components back in so the layout stays compact (far
        // outliers feel it most; the central cluster barely moves).
        for (i, node) in nodes.iter().enumerate() {
            disp[i].0 += (0.5 - node.x) * GRAVITY;
            disp[i].1 += (0.5 - node.y) * GRAVITY;
        }
        // Apply, capped by the current temperature.
        for (i, node) in nodes.iter_mut().enumerate() {
            let (dxv, dyv) = disp[i];
            let d = (dxv * dxv + dyv * dyv).sqrt().max(1e-4);
            let m = d.min(temp);
            node.x += dxv / d * m;
            node.y += dyv / d * m;
        }
        temp -= cool;
    }

    normalize(nodes);
}

/// Rescale node positions to fill `[pad, 1-pad]²`.
fn normalize(nodes: &mut [GraphNode]) {
    const PAD: f64 = 0.06;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for nd in nodes.iter() {
        min_x = min_x.min(nd.x);
        min_y = min_y.min(nd.y);
        max_x = max_x.max(nd.x);
        max_y = max_y.max(nd.y);
    }
    let span_x = (max_x - min_x).max(1e-6);
    let span_y = (max_y - min_y).max(1e-6);
    let scale = 1.0 - 2.0 * PAD;
    for nd in nodes.iter_mut() {
        nd.x = PAD + (nd.x - min_x) / span_x * scale;
        nd.y = PAD + (nd.y - min_y) / span_y * scale;
    }
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(a: &str, b: &str, t: u32) -> CouplingPair {
        CouplingPair {
            a: a.into(),
            b: b.into(),
            together: t,
            degree: 0.0,
        }
    }

    #[test]
    fn empty_graph() {
        let g = build_graph(&[], 10);
        assert!(g.nodes.is_empty() && g.edges.is_empty());
    }

    #[test]
    fn nodes_edges_and_degree() {
        // a-b, a-c → a has degree 2, b and c degree 1; 3 nodes, 2 edges.
        let g = build_graph(&[pair("a", "b", 5), pair("a", "c", 3)], 10);
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edges.len(), 2);
        let a = g.nodes.iter().find(|n| n.file == "a").unwrap();
        assert_eq!(a.degree, 2);
        assert_eq!(g.nodes.iter().find(|n| n.file == "b").unwrap().degree, 1);
    }

    #[test]
    fn positions_within_unit_square() {
        let g = build_graph(
            &[
                pair("a", "b", 5),
                pair("b", "c", 4),
                pair("c", "a", 3),
                pair("c", "d", 2),
            ],
            10,
        );
        for nd in &g.nodes {
            assert!((0.0..=1.0).contains(&nd.x), "x out of range: {}", nd.x);
            assert!((0.0..=1.0).contains(&nd.y), "y out of range: {}", nd.y);
        }
    }

    #[test]
    fn layout_is_deterministic() {
        let pairs = [pair("a", "b", 5), pair("b", "c", 4), pair("a", "c", 3)];
        let g1 = build_graph(&pairs, 10);
        let g2 = build_graph(&pairs, 10);
        assert_eq!(g1, g2);
    }

    #[test]
    fn max_edges_caps_the_graph() {
        let pairs = [pair("a", "b", 5), pair("c", "d", 4), pair("e", "f", 3)];
        let g = build_graph(&pairs, 1);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.nodes.len(), 2);
    }
}
