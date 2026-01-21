# Forge Agent Guidelines

## Specifications

**IMPORTANT:** Before implementing any feature, consult the specifications in `specs/README.md`. After a feature is successfully implemented and tested, update `specs/README.md` with concise additions

- **Assume NOT implemented.** Many specs describe planned features that may not yet exist in the codebase.
- **Check the codebase first.** Before concluding something is or isn't implemented, search the actual code. Specs describe intent; code describes reality.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Spec index:** `specs/README.md` lists all specifications organized by category (core, LLM, security, etc.).

## Commands

### Build
```bash
source ~/.cargo/env && cargo build --workspace
```

### Test
```bash
source ~/.cargo/env && cargo test --workspace
```

### Lint
```bash
source ~/.cargo/env && cargo clippy --workspace -- -D warnings
```

### Format
```bash
source ~/.cargo/env && cargo fmt
```

### Format Check
```bash
source ~/.cargo/env && cargo fmt --check
```

