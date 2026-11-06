@file:OptIn(ExperimentalWasmDsl::class)

import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl

plugins {
    kotlin("multiplatform") version "2.5.0-Beta2"
    kotlin("jvm") version "2.5.0-Beta2" apply false
}

repositories { mavenCentral() }

evaluationDependsOn(":generator")

// The IR from `ts-gen --target ir` (see run.sh); override with -Pir=<file>.
val irFile: Provider<RegularFile> = providers.gradleProperty("ir")
    .map { layout.projectDirectory.file(it) }
    .orElse(layout.buildDirectory.file("worker.ir"))
val bindingsDir = layout.buildDirectory.dir("generated/bindings")

// Runs the generator (the :generator subproject) on the IR.
val generateBindings by tasks.registering(JavaExec::class) {
    val main = project(":generator").extensions.getByType<SourceSetContainer>()["main"]
    classpath = main.runtimeClasspath
    mainClass.set("tsgen.kotlin.MainKt")
    inputs.file(irFile)
    outputs.dir(bindingsDir)
    argumentProviders += CommandLineArgumentProvider {
        listOf(irFile.get().asFile.path, bindingsDir.get().asFile.path, "--verbatim-names")
    }
}

kotlin {
    wasmJs {
        outputModuleName.set("worker")
        nodejs()
        binaries.executable()
    }
    sourceSets {
        wasmJsMain {
            kotlin.srcDir(generateBindings)
            dependencies {
                implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
            }
        }
    }
}
