# HONDA-ONEROM-EDITOR

Version minimale publiee pour eviter les dossiers sensibles.

## Contenu

- minimal-run/onerom-honda-edition
	- binaire Linux x86_64 pret a executer
- minimal-build/
	- strict minimum des sources necessaires pour recompiler onerom-honda-edition

## Execution Linux directe

./minimal-run/onerom-honda-edition

## Recompiler depuis les sources minimales

cargo build --release --manifest-path minimal-build/docs/Cargo.toml

Binaire genere:

minimal-build/docs/target/release/onerom-honda-edition

## Notes

- Cette version retire le gros snapshot precedent.
- Seuls les sous-dossiers Rust necessaires au build sont conserves:
	- rust/cli
	- rust/config
	- rust/fw
	- rust/gen
	- rust/sdrr-fw-parser
