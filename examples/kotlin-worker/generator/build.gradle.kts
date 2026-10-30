plugins {
    kotlin("jvm")
    application
}

repositories { mavenCentral() }

// A full JDK, provisioned by the foojay resolver (settings.gradle.kts)
// where only a JRE is installed.
kotlin { jvmToolchain(21) }

application { mainClass.set("tsgen.kotlin.MainKt") }
