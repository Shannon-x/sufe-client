package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.ForgetPasswordArgs
import com.xboard.client.ui.components.LabeledTextField
import com.xboard.client.ui.components.PrimaryButton
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.AuthState
import kotlinx.coroutines.delay

@Composable
fun ForgetPasswordScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val authState by viewModel.authState.collectAsState()
    val ctx = LocalContext.current

    var email by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var confirm by rememberSaveable { mutableStateOf("") }
    var localError by remember { mutableStateOf<String?>(null) }
    var resendCooldown by remember { mutableStateOf(0) }

    val submitting = authState is AuthState.Submitting

    LaunchedEffect(resendCooldown) {
        if (resendCooldown > 0) { delay(1_000); resendCooldown -= 1 }
    }

    ScreenScaffold(title = stringResource(R.string.forget_heading), onBack = onBack) { padded ->
        ScrollableColumn(modifier = padded, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(
                text = stringResource(R.string.forget_tagline),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            LabeledTextField(
                label = stringResource(R.string.forget_email),
                value = email,
                onValueChange = { email = it; localError = null },
                keyboardType = KeyboardType.Email,
                enabled = !submitting,
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                LabeledTextField(
                    label = stringResource(R.string.forget_code),
                    value = code,
                    onValueChange = { code = it; localError = null },
                    keyboardType = KeyboardType.Number,
                    enabled = !submitting,
                    modifier = Modifier.weight(1f),
                )
                OutlinedButton(
                    onClick = {
                        if (email.isBlank()) {
                            localError = ctx.getString(R.string.forget_fill_all)
                            return@OutlinedButton
                        }
                        viewModel.sendEmailCode(email) { resendCooldown = 60 }
                    },
                    enabled = !submitting && resendCooldown == 0,
                ) {
                    Text(
                        text = if (resendCooldown > 0)
                            stringResource(R.string.register_code_resend, resendCooldown)
                        else stringResource(R.string.register_code_send),
                    )
                }
            }

            LabeledTextField(
                label = stringResource(R.string.forget_password),
                value = password,
                onValueChange = { password = it; localError = null },
                isPassword = true,
                enabled = !submitting,
            )
            LabeledTextField(
                label = stringResource(R.string.forget_password_confirm),
                value = confirm,
                onValueChange = { confirm = it; localError = null },
                isPassword = true,
                enabled = !submitting,
            )

            if (localError != null) {
                Text(
                    text = localError!!,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            PrimaryButton(
                text = if (submitting) stringResource(R.string.forget_submitting)
                    else stringResource(R.string.forget_submit),
                submitting = submitting,
                onClick = {
                    if (email.isBlank() || code.isBlank() || password.isBlank() || confirm.isBlank()) {
                        localError = ctx.getString(R.string.forget_fill_all)
                        return@PrimaryButton
                    }
                    if (password.length < 8) {
                        localError = ctx.getString(R.string.register_password_too_short)
                        return@PrimaryButton
                    }
                    if (password != confirm) {
                        localError = ctx.getString(R.string.register_password_mismatch)
                        return@PrimaryButton
                    }
                    viewModel.forgetPassword(
                        ForgetPasswordArgs(
                            email = email.trim(),
                            password = password,
                            emailCode = code,
                            recaptcha = null,
                            turnstile = null,
                        ),
                        onDone = onBack,
                    )
                },
            )
        }
    }
}
