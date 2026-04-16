package com.thomas.mvp

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpStatusCode
import io.ktor.http.contentType
import java.nio.ByteBuffer
import java.nio.ByteOrder

data class CustomizeResult(
    val statusCode: Int,
    val body: String,
)

class CustomizeClient(serverBaseUrl: String) : AutoCloseable {
    private val customizeUrl = serverBaseUrl.trimEnd('/') + "/customize"
    private val httpClient = HttpClient(CIO) {
        expectSuccess = false
    }

    suspend fun postWeights(weights: IntArray): CustomizeResult {
        val bodyBytes = ByteBuffer.allocate(weights.size * Int.SIZE_BYTES)
            .order(ByteOrder.LITTLE_ENDIAN)
            .apply {
                for (weight in weights) {
                    putInt(weight)
                }
            }
            .array()

        val response = httpClient.post(customizeUrl) {
            contentType(ContentType.Application.OctetStream)
            setBody(bodyBytes)
        }
        val responseBody = response.bodyAsText()
        if (response.status != HttpStatusCode.OK) {
            error(
                "POST $customizeUrl failed with HTTP ${response.status.value}: " +
                    responseBody.ifBlank { "<empty body>" }
            )
        }

        return CustomizeResult(
            statusCode = response.status.value,
            body = responseBody,
        )
    }

    override fun close() {
        httpClient.close()
    }
}
