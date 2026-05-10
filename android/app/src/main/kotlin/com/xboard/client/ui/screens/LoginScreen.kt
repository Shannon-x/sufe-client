package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.ui.components.LabeledTextField
import com.xboard.client.ui.components.PrimaryButton
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.AuthState

@Composable
fun LoginScreen(
    viewModel: AppViewModel,
    onRegister: () -> Unit,
    onForget: () -> Unit,
) {
    val authState by viewModel.authState.collectAsState()
    val ctx = LocalContext.current

    var email by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var localError by remember { mutableStateOf<String?>(null) }

    val submitting = authState is AuthState.Submitting

    ScrollableColumn(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
    ) {
        Spacer(modifier = Modifier.height(24.dp))

        Text(
            text = stringResource(R.string.app_name),
            style = MaterialTheme.typography.headlineLarge,
            fontWeight = FontWeight.Bold,
        )
        Text(
            text = stringResource(R.string.app_tagline),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(modifier = Modifier.height(16.dp))

        Text(
            text = stringResource(R.string.login_heading),
            style = MaterialTheme.typography.titleLarge,
        )

        LabeledTextField(
            label = stringResource(R.string.login_email),
            value = email,
            onValueChange = {
                email = it
                localError = null
            },
            keyboardType = KeyboardType.Email,
            enabled = !submitting,
        )

        LabeledTextField(
            label = stringResource(R.string.login_password),
            value = password,
            onValueChange = {
                password = it
                localError = null
            },
            isPassword = true,
            enabled = !submitting,
        )

        if (localError != null) {
            Text(
                text = localError!!,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.fillMaxWidth(),
            )
        }

        PrimaryButton(
            text = if (submitting) stringResource(R.string.login_submitting)
                else stringResource(R.string.login_submit),
            submitting = submitting,
            onClick = {
                val e = email.trim()
                if (e.isEmpty() || password.isEmpty()) {
                    localError = ctx.getString(R.string.login_fill_all)
                    return@PrimaryButton
                }
                viewModel.login(e, password)
            },
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            TextButton(onClick = onRegister, enabled = !submitting) {
                Text(stringResource(R.string.login_to_register))
            }
            TextButton(onClick = onForget, enabled = !submitting) {
                Text(stringResource(R.string.login_to_forget))
            }
        }
    }
}
