package com.thomas.cch_app

import org.apache.arrow.memory.RootAllocator
import org.apache.arrow.vector.BitVector
import org.apache.arrow.vector.Float4Vector
import org.apache.arrow.vector.UInt4Vector
import org.apache.arrow.vector.UInt8Vector
import org.apache.arrow.vector.VarCharVector
import org.apache.arrow.vector.VectorSchemaRoot
import org.apache.arrow.vector.ipc.ArrowFileReader
import org.slf4j.LoggerFactory
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import kotlin.io.path.exists
import kotlin.io.path.isDirectory
import kotlin.io.path.isRegularFile

data class GraphInputs(
    val graphDir: Path,
    val lineGraphDir: Path,
    val originalTravelTime: IntArray,
    val geoDistance: IntArray,
    val wayIndex: IntArray,
    val lineGraphFirstOut: IntArray,
    val lineGraphHead: IntArray,
    val lineGraphBaselineWeights: IntArray,
    val splitMap: IntArray,
    val roadIndex: RoadIndex,
    val arcManifest: ArcManifest,
    val edgeHighway: Array<String>,
) {
    val originalEdgeCount: Int
        get() = originalTravelTime.size

    val lineGraphNodeCount: Int
        get() = lineGraphFirstOut.size - 1

    val lineGraphEdgeCount: Int
        get() = lineGraphHead.size

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (javaClass != other?.javaClass) return false

        other as GraphInputs

        if (graphDir != other.graphDir) return false
        if (lineGraphDir != other.lineGraphDir) return false
        if (!originalTravelTime.contentEquals(other.originalTravelTime)) return false
        if (!geoDistance.contentEquals(other.geoDistance)) return false
        if (!wayIndex.contentEquals(other.wayIndex)) return false
        if (!lineGraphFirstOut.contentEquals(other.lineGraphFirstOut)) return false
        if (!lineGraphHead.contentEquals(other.lineGraphHead)) return false
        if (!lineGraphBaselineWeights.contentEquals(other.lineGraphBaselineWeights)) return false
        if (!splitMap.contentEquals(other.splitMap)) return false
        if (roadIndex != other.roadIndex) return false
        if (arcManifest != other.arcManifest) return false
        if (!edgeHighway.contentEquals(other.edgeHighway)) return false
        if (originalEdgeCount != other.originalEdgeCount) return false
        if (lineGraphNodeCount != other.lineGraphNodeCount) return false
        if (lineGraphEdgeCount != other.lineGraphEdgeCount) return false

        return true
    }

    override fun hashCode(): Int {
        var result = graphDir.hashCode()
        result = 31 * result + lineGraphDir.hashCode()
        result = 31 * result + originalTravelTime.contentHashCode()
        result = 31 * result + geoDistance.contentHashCode()
        result = 31 * result + wayIndex.contentHashCode()
        result = 31 * result + lineGraphFirstOut.contentHashCode()
        result = 31 * result + lineGraphHead.contentHashCode()
        result = 31 * result + lineGraphBaselineWeights.contentHashCode()
        result = 31 * result + splitMap.contentHashCode()
        result = 31 * result + roadIndex.hashCode()
        result = 31 * result + arcManifest.hashCode()
        result = 31 * result + edgeHighway.contentHashCode()
        result = 31 * result + originalEdgeCount
        result = 31 * result + lineGraphNodeCount
        result = 31 * result + lineGraphEdgeCount
        return result
    }
}

internal data class ResolvedGraphDirs(
    val graphDir: Path,
    val lineGraphDir: Path,
)

data class RoadIndex(
    val osmWayIdByWay: LongArray,
    val nameByWay: Array<String>,
    val highwayByWay: Array<String>,
    val firstArcOffsetByWay: IntArray,
    val arcIdsByWay: IntArray,
) {
    val wayCount: Int
        get() = osmWayIdByWay.size

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (javaClass != other?.javaClass) return false

        other as RoadIndex

        if (!osmWayIdByWay.contentEquals(other.osmWayIdByWay)) return false
        if (!nameByWay.contentEquals(other.nameByWay)) return false
        if (!highwayByWay.contentEquals(other.highwayByWay)) return false
        if (!firstArcOffsetByWay.contentEquals(other.firstArcOffsetByWay)) return false
        if (!arcIdsByWay.contentEquals(other.arcIdsByWay)) return false
        if (wayCount != other.wayCount) return false

        return true
    }

    override fun hashCode(): Int {
        var result = osmWayIdByWay.contentHashCode()
        result = 31 * result + nameByWay.contentHashCode()
        result = 31 * result + highwayByWay.contentHashCode()
        result = 31 * result + firstArcOffsetByWay.contentHashCode()
        result = 31 * result + arcIdsByWay.contentHashCode()
        result = 31 * result + wayCount
        return result
    }
}

data class ArcMetadata(
    val arcId: Int,
    val routingWayId: Int,
    val osmWayId: Long,
    val name: String,
    val highway: String,
    val tailLat: Double,
    val tailLon: Double,
    val headLat: Double,
    val headLon: Double,
    val bearingDeg: Double,
    val isAntiparallelToWay: Boolean,
)

class ArcManifest private constructor(
    private val routingWayIdByArc: IntArray,
    private val osmWayIdByArc: LongArray,
    private val nameByArc: Array<String>,
    private val highwayByArc: Array<String>,
    private val tailLatByArc: FloatArray,
    private val tailLonByArc: FloatArray,
    private val headLatByArc: FloatArray,
    private val headLonByArc: FloatArray,
    private val bearingDegByArc: FloatArray,
    private val antiparallelByArc: BooleanArray,
) {
    val size: Int
        get() = routingWayIdByArc.size

    fun requireBaseArcId(arcId: Int) {
        require(arcId in 0 until size) {
            "arc_id $arcId is outside base-arc range 0..${size - 1}"
        }
    }

    fun routingWayId(arcId: Int): Int {
        requireBaseArcId(arcId)
        return routingWayIdByArc[arcId]
    }

    fun osmWayId(arcId: Int): Long {
        requireBaseArcId(arcId)
        return osmWayIdByArc[arcId]
    }

    fun name(arcId: Int): String {
        requireBaseArcId(arcId)
        return nameByArc[arcId]
    }

    fun highway(arcId: Int): String {
        requireBaseArcId(arcId)
        return highwayByArc[arcId]
    }

    fun tailLat(arcId: Int): Double {
        requireBaseArcId(arcId)
        return tailLatByArc[arcId].toDouble()
    }

    fun tailLon(arcId: Int): Double {
        requireBaseArcId(arcId)
        return tailLonByArc[arcId].toDouble()
    }

    fun headLat(arcId: Int): Double {
        requireBaseArcId(arcId)
        return headLatByArc[arcId].toDouble()
    }

    fun headLon(arcId: Int): Double {
        requireBaseArcId(arcId)
        return headLonByArc[arcId].toDouble()
    }

    fun bearingDeg(arcId: Int): Double {
        requireBaseArcId(arcId)
        return bearingDegByArc[arcId].toDouble()
    }

    fun isAntiparallelToWay(arcId: Int): Boolean {
        requireBaseArcId(arcId)
        return antiparallelByArc[arcId]
    }

    fun metadata(arcId: Int): ArcMetadata {
        requireBaseArcId(arcId)
        return ArcMetadata(
            arcId = arcId,
            routingWayId = routingWayIdByArc[arcId],
            osmWayId = osmWayIdByArc[arcId],
            name = nameByArc[arcId],
            highway = highwayByArc[arcId],
            tailLat = tailLatByArc[arcId].toDouble(),
            tailLon = tailLonByArc[arcId].toDouble(),
            headLat = headLatByArc[arcId].toDouble(),
            headLon = headLonByArc[arcId].toDouble(),
            bearingDeg = bearingDegByArc[arcId].toDouble(),
            isAntiparallelToWay = antiparallelByArc[arcId],
        )
    }

    companion object {
        fun fromRows(rows: List<ArcMetadata>): ArcManifest {
            val size = rows.maxOfOrNull { it.arcId }?.plus(1) ?: 0
            val routingWayIdByArc = IntArray(size)
            val osmWayIdByArc = LongArray(size)
            val nameByArc = Array(size) { "" }
            val highwayByArc = Array(size) { "" }
            val tailLatByArc = FloatArray(size)
            val tailLonByArc = FloatArray(size)
            val headLatByArc = FloatArray(size)
            val headLonByArc = FloatArray(size)
            val bearingDegByArc = FloatArray(size)
            val antiparallelByArc = BooleanArray(size)
            val seen = BooleanArray(size)

            for (row in rows) {
                require(row.arcId in 0 until size) { "arc_id ${row.arcId} is out of range for size $size" }
                require(!seen[row.arcId]) { "duplicate arc_id ${row.arcId} in ArcManifest rows" }
                seen[row.arcId] = true
                routingWayIdByArc[row.arcId] = row.routingWayId
                osmWayIdByArc[row.arcId] = row.osmWayId
                nameByArc[row.arcId] = row.name
                highwayByArc[row.arcId] = row.highway
                tailLatByArc[row.arcId] = row.tailLat.toFloat()
                tailLonByArc[row.arcId] = row.tailLon.toFloat()
                headLatByArc[row.arcId] = row.headLat.toFloat()
                headLonByArc[row.arcId] = row.headLon.toFloat()
                bearingDegByArc[row.arcId] = row.bearingDeg.toFloat()
                antiparallelByArc[row.arcId] = row.isAntiparallelToWay
            }

            require(seen.all { it }) {
                val missing = seen.indexOfFirst { !it }
                "ArcManifest rows are missing arc_id $missing"
            }

            return fromArrays(
                routingWayIdByArc,
                osmWayIdByArc,
                nameByArc,
                highwayByArc,
                tailLatByArc,
                tailLonByArc,
                headLatByArc,
                headLonByArc,
                bearingDegByArc,
                antiparallelByArc,
            )
        }

        internal fun fromArrays(
            routingWayIdByArc: IntArray,
            osmWayIdByArc: LongArray,
            nameByArc: Array<String>,
            highwayByArc: Array<String>,
            tailLatByArc: FloatArray,
            tailLonByArc: FloatArray,
            headLatByArc: FloatArray,
            headLonByArc: FloatArray,
            bearingDegByArc: FloatArray,
            antiparallelByArc: BooleanArray,
        ): ArcManifest {
            val size = routingWayIdByArc.size
            require(osmWayIdByArc.size == size)
            require(nameByArc.size == size)
            require(highwayByArc.size == size)
            require(tailLatByArc.size == size)
            require(tailLonByArc.size == size)
            require(headLatByArc.size == size)
            require(headLonByArc.size == size)
            require(bearingDegByArc.size == size)
            require(antiparallelByArc.size == size)
            return ArcManifest(
                routingWayIdByArc = routingWayIdByArc,
                osmWayIdByArc = osmWayIdByArc,
                nameByArc = nameByArc,
                highwayByArc = highwayByArc,
                tailLatByArc = tailLatByArc,
                tailLonByArc = tailLonByArc,
                headLatByArc = headLatByArc,
                headLonByArc = headLonByArc,
                bearingDegByArc = bearingDegByArc,
                antiparallelByArc = antiparallelByArc,
            )
        }
    }
}

object GraphLoader {
    private val logger = LoggerFactory.getLogger(GraphLoader::class.java)

    fun load(inputGraphDir: Path): GraphInputs {
        val resolvedDirs = resolveGraphDirs(inputGraphDir)
        val graphDir = resolvedDirs.graphDir
        val lineGraphDir = resolvedDirs.lineGraphDir
        requireFile(lineGraphDir.resolve("first_out"))
        requireFile(lineGraphDir.resolve("head"))
        requireFile(lineGraphDir.resolve("travel_time"))

        val originalTravelTime = loadU32Vector(graphDir.resolve("travel_time"))
        val geoDistance = loadU32Vector(graphDir.resolve("geo_distance"))
        val wayIndex = loadU32Vector(graphDir.resolve("way"))

        require(originalTravelTime.size == geoDistance.size) {
            "travel_time length (${originalTravelTime.size}) must match geo_distance length (${geoDistance.size})"
        }
        require(originalTravelTime.size == wayIndex.size) {
            "travel_time length (${originalTravelTime.size}) must match way length (${wayIndex.size})"
        }

        val lineGraphFirstOut = loadU32Vector(lineGraphDir.resolve("first_out"))
        val lineGraphHead = loadU32Vector(lineGraphDir.resolve("head"))
        val lineGraphBaselineWeights = loadU32Vector(lineGraphDir.resolve("travel_time"))
        validateCsr("line_graph", lineGraphFirstOut, lineGraphHead)
        require(lineGraphBaselineWeights.size == lineGraphHead.size) {
            "line_graph travel_time length (${lineGraphBaselineWeights.size}) must match line_graph head length (${lineGraphHead.size})"
        }

        val originalEdgeCount = originalTravelTime.size
        val lineGraphNodeCount = lineGraphFirstOut.size - 1
        val splitMapPath = lineGraphDir.resolve("via_way_split_map")
        val splitMap = when {
            splitMapPath.exists() -> loadU32Vector(splitMapPath)
            lineGraphNodeCount == originalEdgeCount -> IntArray(0)
            else -> error(
                "Missing ${splitMapPath.fileName} in $lineGraphDir while line graph node count " +
                    "($lineGraphNodeCount) exceeds original edge count ($originalEdgeCount)"
            )
        }

        require(lineGraphNodeCount == originalEdgeCount + splitMap.size) {
            "line graph node count ($lineGraphNodeCount) must equal original edge count " +
                "($originalEdgeCount) + split node count (${splitMap.size})"
        }
        for ((idx, originalArcId) in splitMap.withIndex()) {
            require(originalArcId in 0 until originalEdgeCount) {
                "via_way_split_map[$idx] = $originalArcId is outside original edge range 0..${originalEdgeCount - 1}"
            }
        }

        val arcManifest = loadRoadArcManifest(graphDir.resolve("road_arc_manifest.arrow"), originalEdgeCount)
        val roadIndex = buildRoadIndex(wayIndex, arcManifest)
        val edgeHighway = Array(originalEdgeCount) { arcId ->
            roadIndex.highwayByWay[wayIndex[arcId]]
        }

        logger.info(
            "loaded graph inputs: graphDir={}, originalEdges={}, lineGraphNodes={}, lineGraphEdges={}, splitNodes={}",
            graphDir,
            originalEdgeCount,
            lineGraphNodeCount,
            lineGraphHead.size,
            splitMap.size,
        )

        return GraphInputs(
            graphDir = graphDir,
            lineGraphDir = lineGraphDir,
            originalTravelTime = originalTravelTime,
            geoDistance = geoDistance,
            wayIndex = wayIndex,
            lineGraphFirstOut = lineGraphFirstOut,
            lineGraphHead = lineGraphHead,
            lineGraphBaselineWeights = lineGraphBaselineWeights,
            splitMap = splitMap,
            roadIndex = roadIndex,
            arcManifest = arcManifest,
            edgeHighway = edgeHighway,
        )
    }

    internal fun resolveGraphDirs(inputGraphDir: Path): ResolvedGraphDirs {
        require(inputGraphDir.exists()) { "Graph path does not exist: $inputGraphDir" }
        require(inputGraphDir.isDirectory()) { "Graph path must be a directory: $inputGraphDir" }

        val directGraphDir = inputGraphDir.takeIf(::looksLikeGraphDir)
        val nestedGraphDir = inputGraphDir.resolve("graph").takeIf(::looksLikeGraphDir)
        val resolved = buildList {
            if (directGraphDir != null) {
                add(
                    ResolvedGraphDirs(
                        graphDir = directGraphDir,
                        lineGraphDir = directGraphDir.resolve("line_graph"),
                    )
                )
                directGraphDir.parent?.let { parent ->
                    add(
                        ResolvedGraphDirs(
                            graphDir = directGraphDir,
                            lineGraphDir = parent.resolve("line_graph"),
                        )
                    )
                }
            }
            if (nestedGraphDir != null) {
                add(
                    ResolvedGraphDirs(
                        graphDir = nestedGraphDir,
                        lineGraphDir = nestedGraphDir.resolve("line_graph"),
                    )
                )
                add(
                    ResolvedGraphDirs(
                        graphDir = nestedGraphDir,
                        lineGraphDir = inputGraphDir.resolve("line_graph"),
                    )
                )
            }
        }.firstOrNull { it.lineGraphDir.isDirectory() }

        requireNotNull(resolved) {
            "Could not resolve graph directory from $inputGraphDir. Expected one of: " +
                "<graph-dir>/first_out with <graph-dir>/line_graph/, " +
                "<dataset-root>/graph/first_out with <dataset-root>/line_graph/, or " +
                "<dataset-root>/graph/first_out with <dataset-root>/graph/line_graph/."
        }
        return resolved
    }

    private fun looksLikeGraphDir(path: Path): Boolean {
        return path.resolve("first_out").exists()
    }

    private fun buildRoadIndex(wayIndex: IntArray, arcManifest: ArcManifest): RoadIndex {
        val wayCount = (wayIndex.maxOrNull() ?: -1) + 1
        val osmWayIdByWay = LongArray(wayCount)
        val nameByWay = Array(wayCount) { "" }
        val highwayByWay = Array(wayCount) { "" }
        val seenWay = BooleanArray(wayCount)
        val counts = IntArray(wayCount)

        for (arcId in wayIndex.indices) {
            val routingWayId = wayIndex[arcId]
            require(routingWayId == arcManifest.routingWayId(arcId)) {
                "way[$arcId] = $routingWayId does not match road_arc_manifest routing_way_id ${arcManifest.routingWayId(arcId)}"
            }
            counts[routingWayId]++

            val osmWayId = arcManifest.osmWayId(arcId)
            val name = arcManifest.name(arcId)
            val highway = arcManifest.highway(arcId)
            if (!seenWay[routingWayId]) {
                seenWay[routingWayId] = true
                osmWayIdByWay[routingWayId] = osmWayId
                nameByWay[routingWayId] = name
                highwayByWay[routingWayId] = highway
            } else {
                require(osmWayIdByWay[routingWayId] == osmWayId) {
                    "routing_way_id $routingWayId has inconsistent osm_way_id values: " +
                        "${osmWayIdByWay[routingWayId]} vs $osmWayId"
                }
                require(nameByWay[routingWayId] == name) {
                    "routing_way_id $routingWayId has inconsistent road names: '${nameByWay[routingWayId]}' vs '$name'"
                }
                require(highwayByWay[routingWayId] == highway) {
                    "routing_way_id $routingWayId has inconsistent highway classes: '${highwayByWay[routingWayId]}' vs '$highway'"
                }
            }
        }

        val firstArcOffsetByWay = IntArray(wayCount + 1)
        for (wayId in 0 until wayCount) {
            firstArcOffsetByWay[wayId + 1] = firstArcOffsetByWay[wayId] + counts[wayId]
        }

        val arcIdsByWay = IntArray(wayIndex.size)
        val cursor = firstArcOffsetByWay.copyOf()
        for (arcId in wayIndex.indices) {
            val wayId = wayIndex[arcId]
            val position = cursor[wayId]
            arcIdsByWay[position] = arcId
            cursor[wayId] = position + 1
        }

        return RoadIndex(
            osmWayIdByWay = osmWayIdByWay,
            nameByWay = nameByWay,
            highwayByWay = highwayByWay,
            firstArcOffsetByWay = firstArcOffsetByWay,
            arcIdsByWay = arcIdsByWay,
        )
    }

    private fun loadRoadArcManifest(path: Path, expectedArcCount: Int): ArcManifest {
        requireFile(path)

        val routingWayIdByArc = IntArray(expectedArcCount)
        val osmWayIdByArc = LongArray(expectedArcCount)
        val nameByArc = Array(expectedArcCount) { "" }
        val highwayByArc = Array(expectedArcCount) { "" }
        val tailLatByArc = FloatArray(expectedArcCount)
        val tailLonByArc = FloatArray(expectedArcCount)
        val headLatByArc = FloatArray(expectedArcCount)
        val headLonByArc = FloatArray(expectedArcCount)
        val bearingDegByArc = FloatArray(expectedArcCount)
        val antiparallelByArc = BooleanArray(expectedArcCount)
        val seenArc = BooleanArray(expectedArcCount)

        var loadedRows = 0

        RootAllocator(Long.MAX_VALUE).use { allocator ->
            Files.newByteChannel(path, StandardOpenOption.READ).use { channel ->
                ArrowFileReader(channel, allocator).use { reader ->
                    while (reader.loadNextBatch()) {
                        val root = reader.vectorSchemaRoot
                        val arcIdVector = requireVector<UInt4Vector>(root, "arc_id")
                        val routingWayIdVector = requireVector<UInt4Vector>(root, "routing_way_id")
                        val osmWayIdVector = requireVector<UInt8Vector>(root, "osm_way_id")
                        val nameVector = requireVector<VarCharVector>(root, "name")
                        val highwayVector = requireVector<VarCharVector>(root, "highway")
                        val tailLatVector = requireVector<Float4Vector>(root, "tail_lat")
                        val tailLonVector = requireVector<Float4Vector>(root, "tail_lon")
                        val headLatVector = requireVector<Float4Vector>(root, "head_lat")
                        val headLonVector = requireVector<Float4Vector>(root, "head_lon")
                        val bearingVector = requireVector<Float4Vector>(root, "bearing_deg")
                        val antiparallelVector = requireVector<BitVector>(root, "is_antiparallel_to_way")

                        for (row in 0 until root.rowCount) {
                            val arcId = unsignedInt(arcIdVector.getObjectNoOverflow(row), "arc_id", row)
                            require(arcId in 0 until expectedArcCount) {
                                "road_arc_manifest row $row has arc_id $arcId outside expected range 0..${expectedArcCount - 1}"
                            }
                            require(!seenArc[arcId]) {
                                "road_arc_manifest contains duplicate arc_id $arcId"
                            }
                            seenArc[arcId] = true

                            routingWayIdByArc[arcId] =
                                unsignedInt(routingWayIdVector.getObjectNoOverflow(row), "routing_way_id", row)
                            osmWayIdByArc[arcId] =
                                requireNotNull(osmWayIdVector.getObjectNoOverflow(row)) {
                                    "road_arc_manifest row $row has null osm_way_id"
                                }.longValueExact()
                            nameByArc[arcId] = requireText(nameVector, "name", row)
                            highwayByArc[arcId] = requireText(highwayVector, "highway", row)
                            tailLatByArc[arcId] = requireFloat(tailLatVector, "tail_lat", row)
                            tailLonByArc[arcId] = requireFloat(tailLonVector, "tail_lon", row)
                            headLatByArc[arcId] = requireFloat(headLatVector, "head_lat", row)
                            headLonByArc[arcId] = requireFloat(headLonVector, "head_lon", row)
                            bearingDegByArc[arcId] = requireFloat(bearingVector, "bearing_deg", row)
                            antiparallelByArc[arcId] =
                                antiparallelVector.getObject(row)
                                    ?: error("road_arc_manifest row $row has null is_antiparallel_to_way")
                            loadedRows++
                        }
                    }
                }
            }
        }

        require(loadedRows == expectedArcCount) {
            "road_arc_manifest row count ($loadedRows) does not match expected original edge count ($expectedArcCount)"
        }
        require(seenArc.all { it }) {
            val missingArcId = seenArc.indexOfFirst { !it }
            "road_arc_manifest is missing arc_id $missingArcId"
        }

        return ArcManifest.fromArrays(
            routingWayIdByArc = routingWayIdByArc,
            osmWayIdByArc = osmWayIdByArc,
            nameByArc = nameByArc,
            highwayByArc = highwayByArc,
            tailLatByArc = tailLatByArc,
            tailLonByArc = tailLonByArc,
            headLatByArc = headLatByArc,
            headLonByArc = headLonByArc,
            bearingDegByArc = bearingDegByArc,
            antiparallelByArc = antiparallelByArc,
        )
    }

    private fun requireFile(path: Path) {
        require(path.exists()) { "Missing required file: $path" }
        require(path.isRegularFile()) { "Expected a file but found a non-file path: $path" }
    }

    private fun validateCsr(label: String, firstOut: IntArray, head: IntArray) {
        require(firstOut.isNotEmpty()) { "$label first_out is empty" }
        require(firstOut[0] == 0) { "$label first_out[0] must be 0, got ${firstOut[0]}" }
        require(firstOut.last() == head.size) {
            "$label first_out sentinel (${firstOut.last()}) must match head length (${head.size})"
        }
        for (idx in 0 until firstOut.lastIndex) {
            require(firstOut[idx] <= firstOut[idx + 1]) {
                "$label first_out must be non-decreasing; found ${firstOut[idx]} > ${firstOut[idx + 1]} at index $idx"
            }
        }
    }

    private fun loadU32Vector(path: Path): IntArray {
        requireFile(path)
        val bytes = Files.readAllBytes(path)
        require(bytes.size % Int.SIZE_BYTES == 0) {
            "File $path has ${bytes.size} bytes, which is not divisible by ${Int.SIZE_BYTES}"
        }
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        val values = IntArray(bytes.size / Int.SIZE_BYTES)
        for (index in values.indices) {
            values[index] = buffer.int
        }
        return values
    }

    private fun unsignedInt(value: Long?, fieldName: String, row: Int): Int {
        val raw = requireNotNull(value) { "road_arc_manifest row $row has null $fieldName" }
        require(raw in 0..Int.MAX_VALUE.toLong()) {
            "road_arc_manifest row $row has $fieldName=$raw, which exceeds Int.MAX_VALUE"
        }
        return raw.toInt()
    }

    private fun requireFloat(vector: Float4Vector, fieldName: String, row: Int): Float {
        return vector.getObject(row)
            ?: error("road_arc_manifest row $row has null $fieldName")
    }

    private fun requireText(vector: VarCharVector, fieldName: String, row: Int): String {
        return vector.getObject(row)?.toString()
            ?: error("road_arc_manifest row $row has null $fieldName")
    }

    private inline fun <reified T> requireVector(root: VectorSchemaRoot, name: String): T {
        val vector = root.getVector(name)
            ?: error("Arrow file is missing required column '$name'")
        require(vector is T) {
            "Arrow column '$name' has unexpected vector type ${vector.javaClass.name}; expected ${T::class.java.name}"
        }
        return vector
    }
}
