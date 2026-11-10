@file:OptIn(ExperimentalWasmDsl::class)

import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl

plugins { kotlin("multiplatform") version "2.5.0-Beta2" }

repositories { mavenCentral() }

kotlin {
    wasmJs {
        outputModuleName.set("probe")
        nodejs()
        binaries.executable()
    }
    // The bindings ts-gen-kotlin writes (see run.sh).
    sourceSets.wasmJsMain { kotlin.srcDir("build/bindings") }
}
