# Internal path implementation

Imported from sugar_path 3.0.0, https://github.com/hyf0/sugar_path (MIT).
LICENSE is retained unchanged. UPSTREAM.json records the original source/test
SHA-256 hashes before module integration.

This directory is compiled directly as `utils::fs_operations::path`, not as a
vendored crate. Cargo only needs memchr and smallvec. Maps to the native
node:path dependency imported by CC 2.1.88 utils/fsOperations.ts and utils/path.ts;
it does not replace physical realpath, filesystem injection, NFC or permissions.

Integration changes:

- Algorithms are organized as ordinary sibling modules; no numbered fragments
  or include-based implementation layer remain. borrowed/owned contain trait
  declarations and implementations, normalize handles normalization, lexical
  handles cwd-independent components, relative/relative_string handle native
  and string relative paths, windows contains platform helpers, slash handles
  separator conversion, and cwd owns ambient directory lookup.
- Cross-module helpers use pub(super); the caller-facing traits remain
  crate-private. Function bodies, platform gates and tests are preserved.
- Crate-root imports become sibling imports. The module is crate-private.
- The never-enabled cached_current_dir branch is removed; each ambient cwd
  lookup observes the current directory.
- Upstream integration tests live in tests/ and run in the product's nextest
  suite. Test macros stay local. Two self-spawning tests use qualified libtest
  names without the crate prefix and require child completion markers.
- Public-crate doctest examples are retained as ignored upstream examples;
  those imports are historical, while executable coverage lives in tests/.
- Cargo packaging and benchmark infrastructure are not imported.

Windows parity corrections (2026-09-21):

- windows_syntax retains ordinary UNC server/share roots before std::path
  parsing, guards join against accidental UNC, and preserves matching-drive
  explicit bases. dirname/basename/is_absolute use Node win32 lexical scans.
  Node's MIT notice is retained in NODE_LICENSE.
- normalize, absolutize and relative share the UNC correction, including
  explicit cwd inputs. Native fs/path consumers use these boundaries directly.
- Tests retain 160 original Windows Bun results and add Windows-only consumer,
  UNC cwd and native surrogate regressions. Cross-compilation does not establish
  native Windows runtime equivalence. Node/Bun disputed device/colon/ambient-drive
  cases remain unresolved; see the research WINDOWS_FIXES.md report.

Common Windows oracle fixes (2026-09-21): windows_lexical resolves arguments
right-to-left without PathBuf::join, normalizes namespace tails lexically and
compares namespace relative components before native Prefix fast paths. Drive
cwd lookup receives only a drive query, never user path tails. Native resolve and
try_absolutize share it. Explicit absolutize_with cross-drive semantics and the
anchored namespace compatibility policy remain intentionally separate. Obsolete
same-drive concatenation and drive-spelling helpers were removed. Recorded
Node/Bun common fixtures and runtime limits: docs/research/sugar-path-bun-2026-09-21/WINDOWS_COMMON_FIXES.md.

Windows upstream tests which assumed native verbatim slash/root semantics are
updated from shared JS lexical oracles. Namespace borrowing assertions are
limited to value semantics; ordinary path borrowing remains tested. Bare
verbatim drive spelling keeps the old Node-shaped behavior where Bun differs.
Native surrogate preservation remains a boundary contract, not a claim of
Node/Bun agreement on lone-surrogate JS strings.

Node authority update (2026-09-21): the user selected Node for all previously
contested Windows rules. windows_normalize implements Node 24.14 colon/reserved
name rules; windows_lexical uses the namespace marker as the root device;
windows_relative implements UTF-16 relative comparison; windows_cwd reads =X:
before process cwd. Native variadic joins concatenate once, including symlink
tails. Prior anchored-device/native-drive compatibility notes above describe
historical behavior and are superseded by this policy. Explicit invalid-cwd
relative_with retains a documented sugar-only cwd-independent extension.
Evidence and current runtime limits: WINDOWS_NODE_AUTHORITY.md in the research
directory. Node 24.14 ships Unicode 17; Rust's Unicode data must be rechecked
when changing either runtime version. Native surrogate spelling remains retained.
