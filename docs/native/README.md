# Native bindings (Swift and Kotlin)

`crates/siafudb-ffi` wraps `siafudb-core` with UniFFI 0.29 (proc-macro style).
Swift and Kotlin sources are **generated** and committed under `bindings/`;
nothing is hand-written beyond packaging (ADR-0002).

## API

- `Database.inMemory()`, `Database.open(path)` (native builds), `Database.fromSnapshot(data)`, `exportSnapshot()`
- `execute(query, params) -> UInt64` and `query(query, params) -> Rows`, with `params: [String: Value]` bound as `$name`
- `Value`: `Null | Bool | Int | Float | Text | Bytes | List`. Results are typed; temporal, map and vector cells arrive as display text for now.
- `kvGet / kvSet / kvDelete`, `changesSince(seq) -> [Change]`, `setMutationTracking(enabled)`
- `DbError`: `Locked`, `Corrupt`, `Query`, `InvalidArgument`, `Engine`

Parameters are never interpolated into query text. `Bytes` and `List` are not yet
accepted as parameters (`InvalidArgument`); NaN/infinity floats are rejected.

## Swift

```swift
import SiafuDB

let db = try Database.open(path: documentsURL.appendingPathComponent("app.siafu").path)
_ = try db.execute(
    query: "CREATE (:Person {name: $name, age: $age})",
    params: ["name": .text(v: userInput), "age": .int(v: 31)])
let rows = try db.query(
    query: "MATCH (p:Person {name: $name}) RETURN p.age",
    params: ["name": .text(v: "Amara")])
if case .int(let age)? = rows.rows.first?.first { print(age) }

do { _ = try Database.open(path: path) }
catch DbError.Locked(let msg) { /* already open elsewhere */ }
```

Build: `scripts/ffi/build-ios.sh` produces `bindings/swift/SiafuDBFFI.xcframework`
(arm64 device, arm64 simulator, arm64 macOS); add `bindings/swift` as a local
Swift package.

## Kotlin

```kotlin
import uniffi.siafudb_ffi.*

val db = Database.open(File(filesDir, "app.siafu").path)
db.execute("CREATE (:Person {name: \$name})", mapOf("name" to Value.Text(userInput)))
val rows = db.query("MATCH (p:Person) RETURN p.name", emptyMap())
val first = (rows.rows.firstOrNull()?.firstOrNull() as? Value.Text)?.v

try { Database.open(path) } catch (e: DbException.Locked) { /* ... */ }
```

Build: `scripts/ffi/build-android.sh` fills `bindings/kotlin/src/main/jniLibs`
(arm64-v8a, armeabi-v7a, x86_64, x86); `bindings/kotlin` is a Gradle Android
library module (JNA dependency included).

Regenerate the committed sources with `scripts/ffi/generate-bindings.sh`
(host build, no Xcode or NDK needed).

## Threading

`Database` is `Send + Sync` and shared by reference; calls from several threads
are safe (the Rust test suite exercises this). Calls are synchronous and block
the caller, so call them off the main/UI thread (a Swift `Task`/actor or a
Kotlin `Dispatchers.IO` coroutine). UniFFI async support was not evaluated.
A file-backed database is locked per file; a second `open` of the same path
fails with `Locked`.

## Unverified

Nothing below was run; the build host had neither Xcode nor the Android NDK.

- `build-ios.sh` and `build-android.sh` (only `bash -n` checked), the XCFramework, and `Package.swift`.
- The Gradle module (plugin and library versions are guesses) and JNA loading `libsiafudb_ffi.so`.
- The generated Swift and Kotlin were never compiled, only grep-checked.
- Behaviour on devices: sandbox file locking, `fsync`, memory, binary size, cold start.
- iOS/Android static libraries compile; a cdylib link fails without Xcode/NDK, as expected.
- Error classification for `Locked`/`Corrupt` matches engine message text, because `siafudb-core` stringifies engine errors; it is covered by tests for locked files and bad snapshots only.
- Intel iOS simulator (x86_64) and 32-bit iOS are not built.
