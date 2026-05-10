package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import com.xboard.client.core.RegisterArgs
import com.xboard.client.ui.components.LabeledTextField
import com.xboard.client.ui.components.PrimaryButton
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.AuthState
import kotlinx.coroutines.delay

@Composable
fun RegisterScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val authState by viewModel.authState.collectAsState()
    val home by viewModel.home.collectAsState()
    val site = home.siteConfig
    val ctx = LocalContext.current

    var email by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var confirm by rememberSaveable { mutableStateOf("") }
    var invite by rememberSaveable { mutableStateOf("") }
    var agreeTos by rememberSaveable { mutableStateOf(false) }
    var localError by remember { mutableStateOf<String?>(null) }
    var resendCooldown by remember { mutableStateOf(0) }

    val submitting = authState is AuthState.Submitting

    LaunchedEffect(resendCooldown) {
        if (resendCooldown > 0) {
            delay(1_000)
            resendCooldown -= 1
        }
    }

    val inviteRequired = site?.isInviteForce == true
    val emailVerifyEnabled = site?.isEmailVerify == true
    val suffixes = site?.emailWhitelistSuffix.orEmpty()

    ScreenScaffold(title = stringResource(R.string.register_heading), onBack = onBack) { padded ->
        ScrollableColumn(
            modifier = padded,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = stringResource(R.string.register_tagline),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (suffixes.isNotEmpty()) {
                Text(
                    text = stringResource(R.string.register_suffix_hint, suffixes.joinToString(", ")),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            LabeledTextField(
                label = stringResource(R.string.register_email),
                value = email,
                onValueChange = { email = it; localError = null },
                keyboardType = KeyboardType.Email,
                enabled = !submitting,
            )

            if (emailVerifyEnabled) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    LabeledTextField(
                        label = stringResource(R.string.register_code),
                        value = code,
                        onValueChange = { code = it; localError = null },
                        keyboardType = KeyboardType.Number,
                        enabled = !submitting,
                        modifier = Modifier.weight(1f),
                    )
                    OutlinedButton(
                        onClick = {
                            if (email.isBlank()) {
                                localError = ctx.getString(R.string.register_invalid_email)
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
            }

            LabeledTextField(
                label = stringResource(R.string.register_password),
                value = password,
                onValueChange = { password = it; localError = null },
                isPassword = true,
                enabled = !submitting,
            )
            LabeledTextField(
                label = stringResource(R.string.register_password_confirm),
                value = confirm,
                onValueChange = { confirm = it; localError = null },
                isPassword = true,
                enabled = !submitting,
            )

            if (inviteRequired || invite.isNotBlank()) {
                LabeledTextField(
                    label = stringResource(R.string.register_invite),
                    value = invite,
                    onValueChange = { invite = it; localError = null },
                    enabled = !submitting,
                    supportingText = if (inviteRequired)
                        stringResource(R.string.register_invite_required) else null,
                )
            }

            site?.tosUrl?.takeIf { it.isNotBlank() }?.let {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = agreeTos, onCheckedChange = { agreeTos = it })
                    Text(stringResource(R.string.register_tos))
                }
            }

            if (localError != null) {
                Text(
                    text = localError!!,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            PrimaryButton(
                text = if (submitting) stringResource(R.string.register_submitting)
                    else stringResource(R.string.register_submit),
                submitting = submitting,
                onClick = {
                    val err = validate(
                        ctx = ctx,
                        email = email,
                        password = password,
                        confirm = confirm,
                        code = code,
                        invite = invite,
                        agreeTos = agreeTos,
                        emailVerifyEnabled = emailVerifyEnabled,
                        inviteRequired = inviteRequired,
                        tosRequired = !site?.tosUrl.isNullOrBlank(),
                        suffixes = suffixes,
                    )
                    if (err != null) {
                        localError = err
                        return@PrimaryButton
                    }
                    viewModel.register(
                        RegisterArgs(
                            email = email.trim(),
                            password = password,
                            emailCode = code,
                            inviteCode = invite.takeIf { it.isNotBlank() },
                            recaptcha = null,
                            turnstile = null,
                        ),
                    )
                },
            )

            Spacer(Modifier.height(8.dp))
            TextButton(onClick = onBack, enabled = !submitting) {
                Text(stringResource(R.string.register_back_to_login))
            }
        }
    }
}

private fun validate(
    ctx: android.content.Context,
    email: String,
    password: String,
    confirm: String,
    code: String,
    invite: String,
    agreeTos: Boolean,
    emailVerifyEnabled: Boolean,
    inviteRequired: Boolean,
    tosRequired: Boolean,
    suffixes: List<String>,
): String? {
    if (email.isBlank() || password.isBlank() || confirm.isBlank()) {
        return ctx.getString(R.string.register_fill_all)
    }
    if (emailVerifyEnabled && code.isBlank()) {
        return ctx.getString(R.string.register_fill_all)
    }
    if (!email.contains('@')) {
        return ctx.getString(R.string.register_invalid_email)
    }
    if (suffixes.isNotEmpty() && suffixes.none { email.endsWith(it, ignoreCase = true) }) {
        return ctx.getString(R.string.register_suffix_mismatch, suffixes.joinToString(", "))
    }
    if (password.length < 8) return ctx.getString(R.string.register_password_too_short)
    if (password != confirm) return ctx.getString(R.string.register_password_mismatch)
    if (inviteRequired && invite.isBlank()) return ctx.getString(R.string.register_invite_required)
    if (tosRequired && !agreeTos) return ctx.getString(R.string.register_must_agree_tos)
    return null
}
