# AlkALive Wave 12 — String Data Sections (Gap 4)

## Objective
Implement real string data sections in the WASM backend, replacing placeholder `i32.const 0` pointers with actual memory offsets into a data section.

## What was implemented

### StringTable and StringEntry types
Added to `wasm_codegen.rs`:
- `StringEntry { text, offset, byte_len }` — one interned string
- `StringTable` — module-wide string interner with deduplication
  - `intern(text) -> u32` — deduplicates by text, returns memory offset
  - `memory_pages() -> u32` — calculates pages needed (1 page = 64KB)
  - `emit_data_section(&mut Module)` — emits WASM data section

### Memory layout
- Address 0..3: null guard (4 zero bytes, sentinel for null string)
- Address 4+: string entries, each = 4-byte LE length prefix + UTF-8 payload + padding to 4-byte alignment
- Memory pages calculated from total string bytes

### Data section emission
- Null guard segment at offset 0
- One active data segment per string entry at its offset
- Length-prefixed UTF-8 encoding with 4-byte alignment

### Codegen changes
- `pre_scan_strings()` walks the AST to collect all string literals before emitting sections
- `compile_expr` for `Lit::Str(s)` calls `strings.intern(s)` and emits `I32Const(offset)`
- Memory section uses `strings.memory_pages()` for the correct page count
- Data section emitted after code section

## Tests: 9 new string_data_tests, 1231 total passed, 0 failed
