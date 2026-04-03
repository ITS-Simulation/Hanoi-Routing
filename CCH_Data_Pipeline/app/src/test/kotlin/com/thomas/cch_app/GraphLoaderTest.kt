package com.thomas.cch_app

import kotlin.io.path.createDirectories
import kotlin.io.path.createFile
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class GraphLoaderTest {
    @Test
    fun resolvesGraphDirWithSiblingLineGraphWhenInputIsGraphDir() {
        val datasetRoot = kotlin.io.path.createTempDirectory("graph-loader-dataset-")
        val graphDir = datasetRoot.resolve("graph").createDirectories()
        val lineGraphDir = datasetRoot.resolve("line_graph").createDirectories()
        graphDir.resolve("first_out").createFile()
        lineGraphDir.resolve("first_out").createFile()

        val resolved = GraphLoader.resolveGraphDirs(graphDir)

        assertEquals(graphDir.normalize(), resolved.graphDir.normalize())
        assertEquals(lineGraphDir.normalize(), resolved.lineGraphDir.normalize())
    }

    @Test
    fun resolvesDatasetRootWithSiblingGraphAndLineGraph() {
        val datasetRoot = kotlin.io.path.createTempDirectory("graph-loader-root-")
        val graphDir = datasetRoot.resolve("graph").createDirectories()
        val lineGraphDir = datasetRoot.resolve("line_graph").createDirectories()
        graphDir.resolve("first_out").createFile()
        lineGraphDir.resolve("first_out").createFile()

        val resolved = GraphLoader.resolveGraphDirs(datasetRoot)

        assertEquals(graphDir.normalize(), resolved.graphDir.normalize())
        assertEquals(lineGraphDir.normalize(), resolved.lineGraphDir.normalize())
    }

    @Test
    fun rejectsPathsWithoutResolvableLineGraph() {
        val graphDir = kotlin.io.path.createTempDirectory("graph-loader-missing-line-")
        graphDir.resolve("first_out").createFile()

        assertFailsWith<IllegalArgumentException> {
            GraphLoader.resolveGraphDirs(graphDir)
        }
    }
}
