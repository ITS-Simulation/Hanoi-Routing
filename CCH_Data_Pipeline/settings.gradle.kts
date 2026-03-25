plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
    id("com.gradle.develocity") version "4.3.2"
}

rootProject.name = "CCH_Data_Pipeline"
include("app", "modeler", "smoother", "simulation")