# Markdown fixture

## Lists and formatting

### Inline syntax

**Bold** / *italic* / ~~struck out~~ / `inline_code` / `**literal** <!-- literal-inline-comment -->`.

1. Ordered first
2. Ordered second
   - Nested bullet
     - Deeper bullet

- Unordered first
- Unordered second
  1. Nested ordered item

- [ ] TASK_PENDING_SENTINEL
- [x] TASK_DONE_SENTINEL

> Quoted text with **bold** and `inline_code`.

## Code and table

```rust
let marker = "FENCED_RUST_SENTINEL";
// <!-- literal-fenced-comment -->
```

```
FENCED_PLAIN_SENTINEL
**literal fenced text**
```

| Syntax | Result |
| --- | --- |
| TABLE_CELL_SENTINEL | **Readable** |
| Inline code | `table_code` |

## Links and visibility

[Example link](https://example.com/markdown) · #123 · @login

![](https://example.com/markdown-fixture-image.png)

Visible before comment. <!-- HIDDEN_COMMENT_SENTINEL --> Visible after comment.

<!--
HIDDEN_BLOCK_COMMENT_SENTINEL
-->

---

End of Markdown fixture.
