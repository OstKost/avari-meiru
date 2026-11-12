package org.enfein.mieru.model

import org.json.JSONObject

data class Profile(
    val server: String = "",
    val serverPort: Int = 8964,
    val transport: String = "TCP",
    val username: String = "",
    val password: String = "",
    val trafficPattern: String? = null
) {
    fun validate(): Result<Unit> {
        if (server.trim().isEmpty()) {
            return Result.failure(IllegalArgumentException("Server address cannot be empty"))
        }
        if (serverPort !in 1..65535) {
            return Result.failure(IllegalArgumentException("Server port must be between 1 and 65535"))
        }
        if (transport != "TCP" && transport != "UDP") {
            return Result.failure(IllegalArgumentException("Transport must be TCP or UDP"))
        }
        if (username.isEmpty()) {
            return Result.failure(IllegalArgumentException("Username cannot be empty"))
        }
        if (password.isEmpty()) {
            return Result.failure(IllegalArgumentException("Password cannot be empty"))
        }
        return Result.success(Unit)
    }

    fun toJson(includePassword: Boolean = true): String {
        val obj = JSONObject()
        obj.put("server", server)
        obj.put("server_port", serverPort)
        obj.put("transport", transport)
        obj.put("username", username)
        if (includePassword) {
            obj.put("password", password)
        }
        if (!trafficPattern.isNullOrBlank()) {
            obj.put("traffic_pattern", trafficPattern)
        }
        return obj.toString(2)
    }

    companion object {
        fun fromJson(jsonStr: String): Result<Profile> {
            return runCatching {
                val obj = JSONObject(jsonStr)
                val server = obj.getString("server")
                val port = obj.getInt("server_port")
                val transport = obj.getString("transport")
                val username = obj.getString("username")
                val password = obj.optString("password", "")
                val pattern = if (obj.has("traffic_pattern") && !obj.isNull("traffic_pattern")) {
                    obj.getString("traffic_pattern")
                } else null

                val profile = Profile(
                    server = server,
                    serverPort = port,
                    transport = transport,
                    username = username,
                    password = password,
                    trafficPattern = pattern
                )
                profile.validate().getOrThrow()
                profile
            }
        }
    }
}
