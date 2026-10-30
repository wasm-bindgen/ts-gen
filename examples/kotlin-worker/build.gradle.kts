@file:OptIn(ExperimentalWasmDsl::class)

import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl

plugins {
    kotlin("multiplatform") version "2.5.0-Beta2"
    kotlin("jvm") version "2.5.0-Beta2" apply false
}

repositories { mavenCentral() }

evaluationDependsOn(":generator")

// The ABI template from `ts-gen --target wat` (see run.sh); override with -Pabi=<file>.
val abiTemplate: Provider<RegularFile> = providers.gradleProperty("abi")
    .map { layout.projectDirectory.file(it) }
    .orElse(layout.buildDirectory.file("worker.abi.wat"))
val bindingsDir = layout.buildDirectory.dir("generated/bindings")

// Runs the generator (the :generator subproject) on the template.
val generateBindings by tasks.registering(JavaExec::class) {
    val main = project(":generator").extensions.getByType<SourceSetContainer>()["main"]
    classpath = main.runtimeClasspath
    mainClass.set("tsgen.kotlin.MainKt")
    inputs.file(abiTemplate)
    outputs.dir(bindingsDir)
    argumentProviders += CommandLineArgumentProvider {
        listOf(abiTemplate.get().asFile.path, bindingsDir.get().asFile.path)
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
