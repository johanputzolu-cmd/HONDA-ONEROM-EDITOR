# HONDA-ONEROM-EDITOR

Ce repo contient:
- le binaire Linux `onerom-honda-edition`
- une copie autonome des sources OneROM necessaires au rebuild dans `onerom-source/`

## Rebuild local sans ouvrir le repo OneROM d'origine

```bash
cd onerom-source/docs
cargo build --release
```

Binaire produit:

```text
onerom-source/docs/target/release/onerom-honda-edition
```

## Structure importante

- `onerom-source/docs/` : app editor GUI Rust (main.rs, Cargo.toml, assets)
- `onerom-source/rust/` : crates Rust references par `docs/Cargo.toml`

Les dossiers `target/` ne sont pas inclus dans `onerom-source/` pour garder le repo propre.
