// Project settings — pulls Compose/AGP/Kotlin plugins from Google + Maven
// Central, locks transitive plugin resolution down to the platforms we
// actually trust. `RepositoriesMode.FAIL_ON_PROJECT_REPOS` makes the build
// blow up loudly if a sub-module ever tries to declare its own repos
// inline (the modern AGP discouraged-pattern).

pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "xboard-client"
include(":app")
