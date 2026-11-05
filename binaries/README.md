The nlprule binaries (`*.bin`) are derived from LanguageTool and licensed under the LGPLv2.1 license. nlprule statically and dynamically links to these binaries. Under LGPLv2.1 §6(a) this does not have any implications on the license of nlprule itself.

The upstream binaries (LanguageTool v5.2) are distributed via Github Releases. As required by LGPLv2.1 a copy of the license is available here: [LICENSE](LICENSE).

## Spanish binaries of this fork (`es/`)

`es/es_tokenizer.bin.gz` and `es/es_rules.bin.gz` are compiled from LanguageTool 6.6
(`grammar.xml` + `style.xml`, `disambiguation.xml`, the `spanish-pos-dict` dictionary) with the
build tooling of this fork. `scripts/download_es.sh` installs them into `storage/`. To rebuild them:

```bash
# LanguageTool 6.6 standalone, unpacked in $LT (https://languagetool.org/download/)
unzip $LT/libs/spanish-pos-dict.jar -d posdict
python build/make_build_dir.py --lt_dir=$LT --lang_code=es \
    --tag_dict_path=posdict/org/languagetool/resource/es/es-ES.dict \
    --tag_info_path=posdict/org/languagetool/resource/es/es-ES.info --out_dir=data/es
cargo build --release -p nlprule --features "bin compile" --bins
RUST_LOG=warn ./target/release/compile --build-dir data/es \
    --tokenizer-out storage/es_tokenizer.bin --rules-out storage/es_rules.bin
./target/release/test_disambiguation -t storage/es_tokenizer.bin
./target/release/test -t storage/es_tokenizer.bin -r storage/es_rules.bin
```

Compiling takes about 12 minutes. Results: 698 disambiguation rules (all pass their examples),
1179 grammar rules (1162 pass all their examples). sha256 of the uncompressed files:

```
864e432941379f557775df22cdc8f7c8d460d336d6cef4e04a0a258f507aee3c  es_tokenizer.bin
a63e318b33db49368f81ee960fa2887995a50897681e9244b617aa97b0b0a40f  es_rules.bin
```
