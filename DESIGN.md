# ortools-rs — Design-Notizen

Rust-Bindings für CP-SAT, die ohne System-Installation von OR-Tools auskommen.

Status: Gerüst steht, baut, Tests grün. CMake-Rezept implementiert und die
Configure-Stufe gegen OR-Tools 9.15 verifiziert. CI-Workflows geschrieben,
Prebuilts noch nicht veröffentlicht.

---

## 0. Warum überhaupt

Auf crates.io liegen ~6 OR-Tools-Crates. Der einzige mit Traktion ist `cp_sat`
(KardinalAI, ~9k Downloads/Monat). Zwei Probleme, beide bestätigt:

**Build.** `cp_sat` sucht `/usr/local`, `/usr`, `/opt/homebrew`, `/opt/ortools`
ab und linkt dynamisch. Im README steht wörtlich, dass man
`RUSTFLAGS='-Clink-arg=-lprotobuf'` setzen muss. Google liefert zwar C++-
Tarballs, aber **pro Distribution** — debian-11/12/sid, ubuntu-20.04/22.04/
24.04, fedora-40/41/42, almalinux, rockylinux, arch, alpine, opensuse, macOS,
VS2022. Nachgesehen im `debian-12`-Tarball: 241 `.so`, 11 `.a`. Vollständig
dynamisch, deshalb die Matrix.

**Abdeckung.** `cp_sat`s Builder hat 1118 Zeilen und deckt Boolesche + lineare
Constraints ab. Es fehlen: der komplette Scheduling-Layer (`interval`,
`no_overlap`, `no_overlap_2d`, `cumulative`), Routing (`circuit`, `routes`),
`table`, `automaton`, `element`, `inverse`, `reservoir`, `int_prod`/`int_div`/
`int_mod`/`abs`, Solution-Callbacks, Assumptions. Und `proto()` gibt nur `&`
zurück — es gibt keinen Weg daran vorbei.

**Warum nicht dort beitragen.** Genau diese Features wurden schon eingereicht:

| PR | Was | Status |
|---|---|---|
| #31 | Solution handler | offen seit Aug 2023 |
| #30 | allowed/forbidden assignments | offen seit Juli 2023 |
| #40 | Interval variables + scheduling | offen seit Feb 2026 |
| #44 | `solve_with_callback` | nach 7 Tagen kommentarlos geschlossen |

#29 (`only_enforce_if`) lag 2,5 Jahre, wurde dann von jemand anderem neu
implementiert (#36) und *das* in zwei Tagen gemerged. Das Crate ist Kardinals
internes Werkzeug auf crates.io — legitim, aber die Prioritäten sind andere.

---

## 1. Aufteilung

```
ortools-src   Binaries. Kein API. links = "ortools".
cpsat         Bindings + Builder.
```

`ortools-src` ist separat nutzbar — auch für `cp_sat` und die anderen
sys-Crates. Statt gegen den Platzhirsch anzutreten, ist die Infrastruktur etwas,
das er übernehmen kann. Und wenn die API-Seite einschläft, bleibt das
Binary-Crate für sich nützlich.

Namenskonvention nach `openssl-src` / `luajit-src`.

---

## 2. Wie ortools-src die Binaries besorgt

Nach dem Vorbild von [cadrum](https://github.com/lzpel/cadrum) (macht dasselbe
für OpenCASCADE):

```
ORTOOLS_ROOT gesetzt?              → nutzen, nichts anfassen
<target-dir>/ortools-9_15-rev1-<t> → Cache-Hit
feature "source"                   → CMake-Build
sonst                              → Prebuilt-Tarball laden
```

Cache liegt im **Cargo-Target-Dir**, nicht in `OUT_DIR` — sonst wirft ein
`cargo clean -p` oder ein Versionsbump einen 200-MB-Download weg. Hergeleitet
über `OUT_DIR.ancestors().nth(4)`.

`artifact_name()` ist die einzige Namensautorität für Release-Tag, Tarball und
Cache-Verzeichnis, damit URL und Zielverzeichnis nicht auseinanderlaufen können.

### Zwei bewusste Abweichungen von cadrum

**SHA256-Pinning.** cadrum lädt 46 MB und prüft nichts — kein Checksum, keine
Signatur. Bei einem Solver, der in Produktionsplanung landet, ist das der Punkt,
an dem jemand im Security-Review aussteigt. `PREBUILT_SHA256` im Crate-Source;
ein nicht gelisteter Target fällt in den Fehlerpfad statt etwas Ungeprüftes zu
laden.

**Link-Manifest im Tarball.** Statisches OR-Tools braucht Abseils Dutzende
Archive in Abhängigkeitsreihenfolge. Diese Liste im `build.rs` zu hardcoden
heißt, sie beim nächsten OR-Tools-Release still falsch zu haben. Also legt jeder
Tarball ein `cpsat-libs.txt` bei (ein Name pro Zeile, in Link-Reihenfolge).
Fremde Trees per `ORTOOLS_ROOT` haben keins und fallen auf
`["ortools", "protobuf"]` zurück — richtig für die üblichen Shared-Installs.

---

## 3. Fallstricke, die schon aufgeschlagen sind

Alle beim Hochziehen des Gerüsts tatsächlich passiert:

**`OR_PROTO_DLL` / `OR_DLL` müssen leer definiert werden.** Die Makros
entstehen nur beim Bau von OR-Tools selbst. Wer die installierten Header
benutzt, muss sie blanken, sonst parst jedes generierte Protobuf-Symbol als
Typname (`variable 'OR_PROTO_DLL TableStruct_…' has initializer but incomplete
type`). Gilt auf **allen** Plattformen, nicht nur MSVC.

**`ortools-src` muss normale Dependency sein, nicht Build-Dependency.** Cargo
propagiert `links`-Metadaten (`DEP_ORTOOLS_*`) und Link-Flags nur über normale
Dependency-Kanten.

**Die Link-Direktiven gehören in `cpsat`, nicht in `ortools-src`.** Cargo hängt
die Link-Flags eines Build-Scripts an *dessen eigenes* Crate. `ortools-src`
exportiert keine referenzierten Symbole, also wirft rustc seine rlib aus dem
Link — und die Direktiven gleich mit. `ortools-src` publiziert deshalb nur
`cargo:libs=…`, `cpsat` macht daraus `rustc-link-lib`.

**`rustc-link-arg` propagiert nicht über Paketgrenzen.** Der wichtigste Fund.
`cargo:rustc-link-lib` landet in den rlib-Metadaten und erreicht jedes abhängige
Binary; `cargo:rustc-link-arg` gilt nur für die Targets des emittierenden
Pakets. Meine Tests und Examples gehören zu `cpsat`, bekamen also alles — ein
fremdes Crate nicht. Aufgefallen erst, als ich ein Consumer-Crate außerhalb des
Workspace gebaut habe: linkt, startet aber nicht
(`libortools.so.9: cannot open shared object file`).

Konsequenzen:

- **rpath** erreicht Konsumenten nicht. Für die ausgelieferten statischen
  Prebuilts egal — da gibt es zur Laufzeit nichts zu finden. Wer `ORTOOLS_ROOT`
  auf einen Shared-Install zeigt, braucht `LD_LIBRARY_PATH`. Steht im README.
- **`--exclude-libs,ALL`** erreicht Konsumenten nicht. Die eigentliche Abwehr
  gegen kollidierende protobuf-Symbole ist aber die zur *Compile*-Zeit in die
  Objekte gebackene versteckte Sichtbarkeit (`-fvisibility=hidden` im Shim,
  `CMAKE_CXX_VISIBILITY_PRESET` in ortools-src) — und die reist mit. Das Flag
  härtet nur noch `cpsat`s eigene Testbinaries. Mein ursprünglicher Kommentar
  behauptete mehr, als das Flag leistet.
- **`--start-group`** wäre für die zirkulären Abseil-Archive nötig, ist aber aus
  demselben Grund unbrauchbar. Stattdessen wird die Bibliotheksliste jetzt
  **zweimal** emittiert; ein zweiter Durchlauf löst auf, was der erste offen
  ließ. Kostet einen zusätzlichen Archiv-Scan. **Noch nicht gegen einen echten
  statischen Build getestet** — das ist der Punkt, an dem es sich zeigen wird.

Dagegen steht jetzt `tests/consumer/`, bewusst außerhalb des Workspace, in
`just consumer` und in CI. Genau diese Klasse Bug fällt sonst erst beim ersten
externen Nutzer auf.

**`lib` vs. `lib64`.** Red-Hat-Derivate installieren nach `lib64` und lassen ein
`lib/` zurück, das nur CMake-Dateien enthält; Debian nutzt `lib`. `probe()` darf
also nicht das erste *existierende* Verzeichnis nehmen, sondern das, in dem
tatsächlich Bibliotheken liegen. Fiel auf, als `just test` auf Fedora das
Fedora-Archiv zog statt des Debian-Tarballs von vorher.

**`-lprotobuf` ist nötig.** `MessageLite::ParseFromArray` und
`SerializeAsString` liegen in libprotobuf; lld löst sie nicht über die
DT_NEEDED von libortools.so auf. Das ist exakt das
`RUSTFLAGS='-Clink-arg=-lprotobuf'` aus `cp_sat`s README, nur an der richtigen
Stelle.

---

## 4. Offene Risiken

**Protobuf-Symbolkonflikt.** libprotobuf hat einen prozessglobalen
`DescriptorPool`, der beim Static-Init pro `.proto`-Dateiname registriert. Zwei
Kopien im selben Prozess → Kollision beim Linken oder Abort zur Laufzeit.
Gegenmaßnahmen sind eingebaut: `-fvisibility=hidden` beim Shim,
`-Wl,--exclude-libs,ALL` beim Linken. Auf macOS bräuchte es zusätzlich
`-unexported_symbols_list`. **Noch nicht real getestet** — dafür braucht es ein
Testprogramm mit zwei unabhängigen protobuf-Kopien.

**Abseil-ABI hängt an Compiler-Flags.** Abseil verspricht keine ABI-Stabilität,
und der ABI hängt am C++-Standard (`absl::string_view` → `std::string_view` ab
C++17 usw.). Prebuilt mit `-std=c++17` gebaut, Shim mit `-std=c++20` kompiliert
→ stille ODR-Verletzung statt Linker-Fehler. Deshalb ist `.std("c++17")` im
`build.rs` hart gesetzt und nicht vom Host geerbt. Muss beim CMake-Build
identisch sein.

**wasm ist vorerst gestrichen.** Bei cadrum ging es, weil OCCT single-threaded
ist. CP-SAT ist ein paralleles Solver-Portfolio — `std::thread`, `absl::Mutex`,
Atomics sind Architektur, nicht Beiwerk. Über `wasm32-wasip1-threads`
vielleicht machbar, aber ein eigenes Projekt.

**Größe.** Mit vollem Solver-Stack (SCIP, SoPlex, COIN-OR, HiGHS, Boost) landet
man bei 150–400 MB entpackt. CP-SAT braucht davon nichts: nur Abseil, protobuf,
re2, zlib. Der CMake-Aufruf schaltet den Rest deshalb ab
(`USE_SCIP=OFF`, `USE_COINOR=OFF`, `USE_HIGHS=OFF`, `USE_PDLP=OFF`,
`BUILD_MATH_OPT=OFF`, `BUILD_FLATZINC=OFF`). Achtung: OR-Tools hat
`BUILD_SHARED_LIBS` per Default **ON**.

**Wartungslast.** OR-Tools releast 2–3×/Jahr. Jedes Release = CI-Matrix über
5–7 Targets neu bauen und hochladen. Bounded und automatisierbar, hört aber nie
auf. Das ist der eigentliche Preis des Projekts.

---

## 5. Warum Intervals ein eigener Typ sind

CP-SAT hat zwei Indexräume. Int- und Bool-Variablen indizieren in
`CpModelProto::variables`. Ein *Interval* ist keine Variable, sondern ein
Constraint — und `no_overlap`/`cumulative` referenzieren es über seinen Index in
`CpModelProto::constraints`.

Die beiden zu vermischen erzeugt Modelle, die validieren und dann das falsche
Problem lösen. `IntervalVar` ist deshalb ein eigener Handle-Typ, der dort, wo
ein `IntVar` erwartet wird, nicht kompiliert.

---

## 6. CI

Zwei Workflows, bewusst getrennt:

- `test.yml` — bei jedem Push/PR. Zieht Googles offizielles C++-Archiv, setzt
  `ORTOOLS_ROOT`, dann fmt + clippy + Tests + Job-Shop auf Linux und macOS.
  Funktioniert **heute**, unabhängig von eigenen Prebuilts.
- `prebuilt.yml` — nur `workflow_dispatch` bzw. Push auf Branch `prebuilt`.
  Fünf Targets, danach Release. Läuft so oft wie OR-Tools releast, also 2–3×
  im Jahr.

Genauso macht es cadrum (Branch `prebuilt` + `workflow_dispatch`, Docker-Cross
für Linux/mingw/wasm, native Runner für macOS und windows-msvc). Zwei Dinge
mache ich anders, beides Reaktion auf konkrete Schwächen dort:

**Tag wird abgeleitet, nicht hartkodiert.** cadrums Workflow hat
`tag_name: occt-8_0_1_rev2` im YAML stehen, mit Kommentar „must match build.rs" —
und steht dort gerade auf `rev2`, während `build.rs` `rev1` sagt. Genau die
Falle. Hier liegen Version und Revision in `crates/ortools-src/version.txt`,
gelesen von `build.rs` (`include_str!`) und vom `name`-Job. Und der Pack-Schritt
leitet gar nichts ab: `build.rs` schreibt den fertigen Artefaktnamen nach
`target/ortools-artifact-name.txt`, CI liest den.

**Checksummen.** cadrums Workflow hat keinen einzigen. Hier berechnet der
Release-Job SHA256, hängt `SHA256SUMS` ans Release und schreibt einen
fertig einfügbaren Rust-Block in die Job-Summary. Weil `build.rs` unbekannte
Digests ablehnt, ist ein Release ohne eingetragene Summen wirkungslos — das ist
Absicht.

**Linux in glibc-2.28-Container** (`manylinux_2_28`). Das ist der Hebel, der
Googles vierzehn Distro-Archive auf ein Tarball pro Architektur eindampft.
macOS und Windows nativ, weil libc++ und MSVC-STL stabile ABIs haben und
Cross-Builds dort Linkfehler erzeugen, die erst beim Konsumenten auftauchen
(cadrum dokumentiert das für windows-msvc als Issue #73).

### Werkzeug

`justfile` + `tools/update-digests.py`. Kein xtask — es sind vier Rezepte, und
`just` ist hier ohnehin überall im Einsatz.

Release-Ablauf: `just prebuilt` (optional, ein Linux-Target lokal im selben
Container zur Rezeptprüfung) → `prebuilt`-Workflow laufen lassen → `just
digests` schreibt die Digests aus dem Release in `build.rs` und formatiert
nach. Bewusst unstaged gelassen, damit man den Diff liest.

Der Workflow nimmt einen `targets`-Parameter: `all`, eine OS-Gruppe (`linux`,
`macos`, `windows`) oder eine Komma-Liste von Triples. Die Matrix wird daraus
per jq gefiltert und über `fromJSON` in den Build-Job gereicht. **Ein Teillauf
veröffentlicht kein Release** — `just digests` ersetzt die Tabelle vollständig,
ein unvollständiges Release würde also die nicht neu gebauten Targets still
entpinnen.

Kosten: auf öffentlichen Repos unbegrenzt gratis. Privat nicht — macOS zählt
10×, Windows 2×, ein voller Lauf also grob 1400 Minuten gegen 2000/Monat.

**Warum `digests.txt` und nicht `SHA256SUMS`:** `build.rs` macht aus einem
Target-Triple den Tarball-Namen, indem es Bindestriche zu Unterstrichen macht.
`x86_64-unknown-linux-gnu` wird zu `x86_64_unknown_linux_gnu` — und das ist
nicht umkehrbar, weil `x86_64` selbst einen Unterstrich enthält. Ein Parser über
Dateinamen produziert `x86-64-unknown-linux-gnu` und damit eine Tabelle, die nie
trifft. Deshalb schreibt jeder Build-Job die Zuordnung selbst hin, wo die Matrix
das echte Triple noch kennt. (Beim Testen genau so aufgeschlagen.)

### Verifiziert

Die Configure-Stufe lief lokal gegen die echten 9.15-Quellen:

- Keine „Manually-specified variables were not used"-Warnung → alle Flagnamen
  korrekt.
- `SCIP support: OFF`, `Build Soplex: OFF`, `HiGHS: OFF`, `PDLP: OFF`,
  `MathOpt: OFF`.
- Geholte Dependencies: `absl`, `bzip2`, `eigen3`, `protobuf`, `re2`, `zlib`.
  Sechs statt der ~20 aus dem Vollausbau.

Ein **vollständiger** Source-Build (30–90 min) ist noch nicht durchgelaufen.

---

## 7. Nächste Schritte

1. Restliche Constraints: `circuit`, `routes`, `table`, `automaton`, `element`,
   `inverse`, `reservoir`, `int_prod`/`int_div`/`int_mod`/`abs`, `lin_max`.
   Mechanisch, je ~30 Zeilen, kein FFI-Risiko.
2. Solution-Callbacks. Der einzige echt schwierige Teil: `extern "C"`-Trampolin
   plus `catch_unwind`, Rust-Seite muss `Send` sein. Genau deshalb hat es keiner.
3. `prebuilt.yml` einmal laufen lassen, Digests aus der Job-Summary in
   `PREBUILT_SHA256` eintragen, dann entfällt `ORTOOLS_ROOT`.
4. Protobuf-Symbolisolation real testen: ein Binary, das `cpsat` **und** eine
   zweite, unabhängige libprotobuf-Kopie zieht. Bis dahin ist
   `-fvisibility=hidden` + `--exclude-libs,ALL` Theorie.
5. Erst dann veröffentlichen. Ein Crate, das bei `cargo build` scheitert, kommt
   nicht zurück.
