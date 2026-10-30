#!/usr/bin/env bash
# Generates Kotlin bindings for each template given (default: every
# tests/snapshots/*/default.wat) and compiles them with an app scaffold,
# in a scratch Gradle project under build/check. Prints OK/FAIL per template.
set -uo pipefail
EX="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$EX/../.." && pwd)"
CHECK="$EX/build/check"
(cd "$EX" && ./gradlew --quiet :generator:installDist) || exit 1
G="$EX/generator/build/install/generator/bin/generator"
mkdir -p "$CHECK"
cp -r "$EX/gradlew" "$EX/gradle" "$EX/gradle.properties" "$CHECK/"
echo 'rootProject.name = "check"' > "$CHECK/settings.gradle.kts"
cat > "$CHECK/build.gradle.kts" <<'KTS'
plugins { kotlin("multiplatform") version "2.5.0-Beta2" }
repositories { mavenCentral() }
kotlin {
    wasmJs { nodejs(); binaries.executable() }
    sourceSets.wasmJsMain {
        kotlin.srcDir("gen")
        dependencies { implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2") }
    }
}
KTS
[ $# -eq 0 ] && set -- "$ROOT"/tests/snapshots/*/default.wat
TEMPLATES=()
for t in "$@"; do TEMPLATES+=("$(realpath "$t")"); done
cd "$CHECK"
for t in "${TEMPLATES[@]}"; do
  name=$(basename "$(dirname "$t")")
  if ! "$G" "$t" gen --scaffold gen/app/App.kt > gen.log 2>&1; then echo "GENFAIL $name: $(tail -1 gen.log)"; continue; fi
  if ./gradlew --quiet compileKotlinWasmJs > kt.log 2>&1; then echo "OK $name"
  else echo "FAIL $name"; grep '^e:' kt.log | head -5 | sed 's/^/    /'; fi
done
