package org.enfein.mieru.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.enfein.mieru.model.Profile
import org.enfein.mieru.model.VpnStatus

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainScreen(
    viewModel: MainViewModel,
    onRequestVpnPermission: () -> Unit
) {
    val profile by viewModel.profile.collectAsState()
    val vpnState by viewModel.vpnState.collectAsState()
    val logs by viewModel.logs.collectAsState()
    val errorMessage by viewModel.errorMessage.collectAsState()

    var showJsonDialog by remember { mutableStateOf(false) }
    var passwordVisible by remember { mutableStateOf(false) }
    val clipboardManager = LocalClipboardManager.current

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Mieru Client", style = MaterialTheme.typography.titleLarge) },
                actions = {
                    IconButton(onClick = { showJsonDialog = true }) {
                        Icon(Icons.Default.Code, contentDescription = "JSON Profile")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background
                )
            )
        },
        containerColor = MaterialTheme.colorScheme.background
    ) { paddingValues ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // Status Card
            item {
                StatusCard(vpnState = vpnState)
            }

            // Local error banner if any
            if (errorMessage != null) {
                item {
                    Card(
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(Icons.Default.Error, contentDescription = null, tint = MaterialTheme.colorScheme.error)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text(
                                text = errorMessage ?: "",
                                color = MaterialTheme.colorScheme.onErrorContainer,
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.weight(1f)
                            )
                            IconButton(onClick = { viewModel.clearError() }) {
                                Icon(Icons.Default.Close, contentDescription = "Dismiss")
                            }
                        }
                    }
                }
            }

            // Profile Configuration Form
            item {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        Text(
                            text = "Server Profile",
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.primary
                        )

                        // Server Address
                        OutlinedTextField(
                            value = profile.server,
                            onValueChange = { viewModel.updateProfile(profile.copy(server = it)) },
                            label = { Text("Server Host or IP") },
                            placeholder = { Text("e.g. 192.0.2.1 or vpn.example.com") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                        )

                        // Port & Transport Row
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            OutlinedTextField(
                                value = if (profile.serverPort > 0) profile.serverPort.toString() else "",
                                onValueChange = {
                                    val port = it.toIntOrNull() ?: 0
                                    viewModel.updateProfile(profile.copy(serverPort = port))
                                },
                                label = { Text("Port") },
                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                                modifier = Modifier.weight(1f),
                                singleLine = true,
                                enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                            )

                            // Transport Switch (TCP / UDP)
                            Column(modifier = Modifier.weight(1f)) {
                                Text("Transport", style = MaterialTheme.typography.labelMedium)
                                Spacer(modifier = Modifier.height(4.dp))
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    FilterChip(
                                        selected = profile.transport == "TCP",
                                        onClick = { viewModel.updateProfile(profile.copy(transport = "TCP")) },
                                        label = { Text("TCP") },
                                        enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                                    )
                                    Spacer(modifier = Modifier.width(8.dp))
                                    FilterChip(
                                        selected = profile.transport == "UDP",
                                        onClick = { viewModel.updateProfile(profile.copy(transport = "UDP")) },
                                        label = { Text("UDP") },
                                        enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                                    )
                                }
                            }
                        }

                        // Username
                        OutlinedTextField(
                            value = profile.username,
                            onValueChange = { viewModel.updateProfile(profile.copy(username = it)) },
                            label = { Text("Username") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                        )

                        // Password (with visibility toggle)
                        OutlinedTextField(
                            value = profile.password,
                            onValueChange = { viewModel.updateProfile(profile.copy(password = it)) },
                            label = { Text("Password") },
                            visualTransformation = if (passwordVisible) VisualTransformation.None else PasswordVisualTransformation(),
                            trailingIcon = {
                                val image = if (passwordVisible) Icons.Default.Visibility else Icons.Default.VisibilityOff
                                IconButton(onClick = { passwordVisible = !passwordVisible }) {
                                    Icon(image, contentDescription = if (passwordVisible) "Hide password" else "Show password")
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                        )

                        // Traffic Pattern (Optional)
                        OutlinedTextField(
                            value = profile.trafficPattern ?: "",
                            onValueChange = {
                                viewModel.updateProfile(profile.copy(trafficPattern = it.takeIf { s -> s.isNotEmpty() }))
                            },
                            label = { Text("Traffic Pattern (Base64, Optional)") },
                            placeholder = { Text("Optional base64 protobuf") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            enabled = vpnState.status == VpnStatus.DISCONNECTED || vpnState.status == VpnStatus.ERROR
                        )
                    }
                }
            }

            // Connection Action Button
            item {
                when (vpnState.status) {
                    VpnStatus.CONNECTED -> {
                        Button(
                            onClick = { viewModel.disconnect() },
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFEF4444)),
                            shape = RoundedCornerShape(10.dp)
                        ) {
                            Icon(Icons.Default.PowerSettingsNew, contentDescription = null)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Disconnect", style = MaterialTheme.typography.titleMedium)
                        }
                    }
                    VpnStatus.CONNECTING -> {
                        Button(
                            onClick = { viewModel.disconnect() },
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                            colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFF59E0B)),
                            shape = RoundedCornerShape(10.dp)
                        ) {
                            CircularProgressIndicator(modifier = Modifier.size(20.dp), color = Color.White, strokeWidth = 2.dp)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Connecting… (Cancel)", style = MaterialTheme.typography.titleMedium)
                        }
                    }
                    VpnStatus.DISCONNECTING -> {
                        Button(
                            onClick = {},
                            enabled = false,
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                            shape = RoundedCornerShape(10.dp)
                        ) {
                            CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Disconnecting…", style = MaterialTheme.typography.titleMedium)
                        }
                    }
                    else -> {
                        Button(
                            onClick = {
                                onRequestVpnPermission()
                            },
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                            shape = RoundedCornerShape(10.dp)
                        ) {
                            Icon(Icons.Default.VpnKey, contentDescription = null)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Connect", style = MaterialTheme.typography.titleMedium)
                        }
                    }
                }
            }

            // Logs Console Card
            item {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = Color(0xFF030712)),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    Column(modifier = Modifier.padding(12.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                text = "Activity Logs (${logs.size})",
                                style = MaterialTheme.typography.labelLarge,
                                color = MaterialTheme.colorScheme.primary
                            )
                            TextButton(onClick = { viewModel.clearLogs() }) {
                                Text("Clear", style = MaterialTheme.typography.labelSmall)
                            }
                        }

                        val listState = rememberLazyListState()
                        LaunchedEffect(logs.size) {
                            if (logs.isNotEmpty()) {
                                listState.animateScrollToItem(logs.size - 1)
                            }
                        }

                        LazyColumn(
                            state = listState,
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 120.dp, max = 220.dp)
                                .background(Color(0xFF030712))
                                .padding(vertical = 4.dp)
                        ) {
                            if (logs.isEmpty()) {
                                item {
                                    Text(
                                        text = "No logs yet.",
                                        color = Color.DarkGray,
                                        fontSize = 12.sp,
                                        fontFamily = FontFamily.Monospace
                                    )
                                }
                            } else {
                                items(logs) { entry ->
                                    val color = when (entry.level) {
                                        "ERROR" -> Color(0xFFEF4444)
                                        "WARN" -> Color(0xFFF59E0B)
                                        else -> Color(0xFF94A3B8)
                                    }
                                    Text(
                                        text = entry.format(),
                                        color = color,
                                        fontSize = 11.sp,
                                        fontFamily = FontFamily.Monospace,
                                        lineHeight = 15.sp
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // JSON Import/Export Dialog
    if (showJsonDialog) {
        JsonProfileDialog(
            profile = profile,
            onDismiss = { showJsonDialog = false },
            onImport = { json ->
                viewModel.importProfileJson(json)
                showJsonDialog = false
            },
            onCopy = { text ->
                clipboardManager.setText(AnnotatedString(text))
            }
        )
    }
}

@Composable
private fun StatusCard(vpnState: org.enfein.mieru.model.VpnState) {
    val (statusColor, statusText, statusIcon) = when (vpnState.status) {
        VpnStatus.DISCONNECTED -> Triple(Color(0xFF64748B), "Disconnected", Icons.Default.CloudOff)
        VpnStatus.CONNECTING -> Triple(Color(0xFFF59E0B), "Connecting…", Icons.Default.Sync)
        VpnStatus.CONNECTED -> Triple(Color(0xFF10B981), "Connected", Icons.Default.CheckCircle)
        VpnStatus.DISCONNECTING -> Triple(Color(0xFFF97316), "Disconnecting…", Icons.Default.Sync)
        VpnStatus.ERROR -> Triple(Color(0xFFEF4444), "Connection Error", Icons.Default.Error)
    }

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(12.dp)
    ) {
        Row(
            modifier = Modifier
                .padding(16.dp)
                .fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier
                    .size(44.dp)
                    .background(statusColor.copy(alpha = 0.2f), shape = RoundedCornerShape(22.dp)),
                contentAlignment = Alignment.Center
            ) {
                Icon(statusIcon, contentDescription = null, tint = statusColor, modifier = Modifier.size(24.dp))
            }
            Spacer(modifier = Modifier.width(16.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(text = "VPN Status", style = MaterialTheme.typography.labelMedium, color = Color(0xFF94A3B8))
                Text(text = statusText, style = MaterialTheme.typography.titleLarge, color = statusColor)
                if (vpnState.message.isNotEmpty()) {
                    Text(
                        text = vpnState.message,
                        style = MaterialTheme.typography.bodySmall,
                        color = if (vpnState.status == VpnStatus.ERROR) Color(0xFFEF4444) else Color(0xFFCBD5E1)
                    )
                }
            }
        }
    }
}

@Composable
private fun JsonProfileDialog(
    profile: Profile,
    onDismiss: () -> Unit,
    onImport: (String) -> Unit,
    onCopy: (String) -> Unit
) {
    var jsonText by remember { mutableStateOf(profile.toJson(includePassword = true)) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Profile JSON (profile.schema.json)") },
        text = {
            Column(modifier = Modifier.fillMaxWidth()) {
                Text(
                    text = "Paste a profile JSON to import, or copy the current profile:",
                    style = MaterialTheme.typography.bodySmall,
                    color = Color.Gray
                )
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedTextField(
                    value = jsonText,
                    onValueChange = { jsonText = it },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(200.dp),
                    textStyle = LocalTextStyle.current.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp
                    )
                )
            }
        },
        confirmButton = {
            Button(onClick = { onImport(jsonText) }) {
                Text("Import")
            }
        },
        dismissButton = {
            Row {
                TextButton(onClick = { onCopy(jsonText) }) {
                    Text("Copy")
                }
                TextButton(onClick = onDismiss) {
                    Text("Cancel")
                }
            }
        }
    )
}
