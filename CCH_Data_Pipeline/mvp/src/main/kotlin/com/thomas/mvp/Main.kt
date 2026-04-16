package com.thomas.mvp

import com.github.ajalt.clikt.core.CliktCommand
import com.github.ajalt.clikt.core.main
import com.github.ajalt.clikt.parameters.options.default
import com.github.ajalt.clikt.parameters.options.flag
import com.github.ajalt.clikt.parameters.options.option
import com.github.ajalt.clikt.parameters.options.required
import com.github.ajalt.clikt.parameters.types.double
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.slf4j.LoggerFactory
import java.nio.file.Path

private const val LOOP_INTERVAL_MILLIS = 30_000L

class LiveWeightMvpCommand : CliktCommand(name = "mvp") {
    private val graphDir by option("--graph-dir", help = "Original graph directory (or dataset root containing graph/)")
        .required()
    private val cameras by option("--cameras", help = "Optional cameras.yaml path")
    private val server by option("--server", help = "Customize server base URL, e.g. http://localhost:9080")
        .required()
    private val hour by option("--hour", help = "Simulation hour in [0,24), used as the one-shot hour or loop start time")
        .double()
        .required()
    private val loop by option("--loop", help = "Continuously refresh weights every 30 seconds")
        .flag(default = false)
    private val timeAccel by option("--time-accel", help = "Simulation-seconds per real second in loop mode")
        .double()
        .default(60.0)

    override fun run() = runBlocking {
        require(timeAccel > 0.0) { "--time-accel must be > 0, got $timeAccel" }

        val inputs = GraphLoader.load(Path.of(graphDir))
        val fanOut = LineGraphFanOut.build(
            originalEdgeCount = inputs.originalEdgeCount,
            lineGraphNodeCount = inputs.lineGraphNodeCount,
            lineGraphHead = inputs.lineGraphHead,
            lineGraphBaselineWeights = inputs.lineGraphBaselineWeights,
            originalTravelTime = inputs.originalTravelTime,
            splitMap = inputs.splitMap,
        )

        val cameraConfig = cameras?.let { CameraConfigLoader.load(Path.of(it)) } ?: CameraConfig.empty()
        val resolver = CameraResolver(inputs.arcManifest, inputs.originalEdgeCount)
        val resolvedCameras = resolver.resolveAll(cameraConfig.cameras)
        val anchorProfiles = LinkedHashMap<Int, SpeedProfile>(resolvedCameras.size)
        val firstCameraByArc = mutableMapOf<Int, ResolvedCamera>()
        for (resolved in resolvedCameras) {
            val previous = firstCameraByArc.putIfAbsent(resolved.arcId, resolved)
            if (previous != null) {
                throw IllegalArgumentException(
                    "Camera ${resolved.camera.id} ('${resolved.camera.label}') and camera ${previous.camera.id} " +
                        "('${previous.camera.label}') both resolve to arc_id=${resolved.arcId}. " +
                        "Use one camera entry per directed arc in a runtime config."
                )
            }
            anchorProfiles[resolved.arcId] =
                requireNotNull(cameraConfig.profiles[resolved.camera.profileName]) {
                    "Resolved camera ${resolved.camera.id} references unknown profile '${resolved.camera.profileName}'"
                }
        }
        val expander = CameraProfileExpander(inputs.roadIndex, inputs.arcManifest, inputs.wayIndex)
        val cameraProfiles = expander.expand(anchorProfiles)

        logger.info(
            "camera propagation: anchorArcs={}, expandedArcs={}",
            anchorProfiles.size,
            cameraProfiles.size,
        )

        logger.info(
            "startup complete: graphDir={}, originalEdges={}, lineGraphEdges={}, profiles={}, cameras={}, coveredEdges={}",
            inputs.graphDir,
            inputs.originalEdgeCount,
            inputs.lineGraphEdgeCount,
            cameraConfig.profiles.size,
            cameraConfig.cameras.size,
            cameraProfiles.size,
        )

        CustomizeClient(server).use { client ->
            if (loop) {
                runLoop(client, inputs, fanOut, cameraProfiles)
            } else {
                pushOnce(client, inputs, fanOut, cameraProfiles, hour)
            }
        }
    }

    private suspend fun pushOnce(
        client: CustomizeClient,
        inputs: GraphInputs,
        fanOut: LineGraphFanOut,
        cameraProfiles: Map<Int, SpeedProfile>,
        hour: Double,
    ) {
        val generator = WeightGenerator(inputs, fanOut, cameraProfiles)
        val weights = generator.generateWeights(hour)
        generator.logSummary(hour, weights)
        val result = client.postWeights(weights)
        logger.info(
            "customize posted successfully: statusCode={}, body={}",
            result.statusCode,
            result.body.ifBlank { "<empty body>" },
        )
    }

    private suspend fun runLoop(
        client: CustomizeClient,
        inputs: GraphInputs,
        fanOut: LineGraphFanOut,
        cameraProfiles: Map<Int, SpeedProfile>,
    ) {
        val generator = WeightGenerator(inputs, fanOut, cameraProfiles)
        val startHour = WeightGenerator.normalizeHour(hour)
        val loopStartNanos = System.nanoTime()
        var iteration = 0L

        while (true) {
            val elapsedSeconds = (System.nanoTime() - loopStartNanos) / 1_000_000_000.0
            val currentHour = WeightGenerator.normalizeHour(startHour + elapsedSeconds * timeAccel / 3600.0)
            val weights = generator.generateWeights(currentHour)
            generator.logSummary(currentHour, weights)
            val result = client.postWeights(weights)
            logger.info(
                "loop iteration={} customize posted successfully: simHour={}, statusCode={}, body={}",
                iteration,
                currentHour,
                result.statusCode,
                result.body.ifBlank { "<empty body>" },
            )
            iteration++

            try {
                withContext(Dispatchers.IO) {
                    Thread.sleep(LOOP_INTERVAL_MILLIS)
                }
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
                logger.info("loop interrupted, shutting down")
                return
            }
        }
    }

    companion object {
        private val logger = LoggerFactory.getLogger(LiveWeightMvpCommand::class.java)
    }
}

fun main(args: Array<String>) = LiveWeightMvpCommand().main(args)
