package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.SaveTicketArgs
import com.xboard.client.vm.AppViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TicketComposerSheet(viewModel: AppViewModel, onDismiss: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val ctx = LocalContext.current

    var subject by remember { mutableStateOf("") }
    var level by remember { mutableStateOf(0) }
    var message by remember { mutableStateOf("") }
    var localError by remember { mutableStateOf<String?>(null) }
    var submitting by remember { mutableStateOf(false) }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = stringResource(R.string.tickets_composer_new),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = 8.dp),
            )

            OutlinedTextField(
                value = subject,
                onValueChange = { subject = it; localError = null },
                label = { Text(stringResource(R.string.tickets_composer_subject)) },
                placeholder = { Text(stringResource(R.string.tickets_composer_subject_placeholder)) },
                singleLine = true,
                enabled = !submitting,
                modifier = Modifier.fillMaxWidth(),
            )

            Text(stringResource(R.string.tickets_composer_level), style = MaterialTheme.typography.labelLarge)
            Row(
                modifier = Modifier.selectableGroup(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                LevelOption(label = stringResource(R.string.tickets_level_low), selected = level == 0) {
                    level = 0
                }
                LevelOption(label = stringResource(R.string.tickets_level_normal), selected = level == 1) {
                    level = 1
                }
                LevelOption(label = stringResource(R.string.tickets_level_high), selected = level == 2) {
                    level = 2
                }
            }

            OutlinedTextField(
                value = message,
                onValueChange = { message = it; localError = null },
                label = { Text(stringResource(R.string.tickets_composer_message)) },
                placeholder = { Text(stringResource(R.string.tickets_composer_message_placeholder)) },
                modifier = Modifier.fillMaxWidth(),
                minLines = 4,
                enabled = !submitting,
            )

            if (localError != null) {
                Text(
                    text = localError!!,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedButton(
                    onClick = onDismiss,
                    enabled = !submitting,
                    modifier = Modifier.weight(1f),
                ) {
                    Text(stringResource(R.string.tickets_composer_cancel))
                }
                Button(
                    onClick = {
                        if (subject.isBlank() || message.isBlank()) {
                            localError = ctx.getString(R.string.tickets_composer_fill_all)
                            return@Button
                        }
                        submitting = true
                        viewModel.saveTicket(
                            SaveTicketArgs(
                                subject = subject.trim(),
                                level = level,
                                message = message.trim(),
                            ),
                        ) {
                            submitting = false
                            onDismiss()
                        }
                    },
                    enabled = !submitting,
                    modifier = Modifier.weight(1f),
                ) {
                    if (submitting) {
                        CircularProgressIndicator(
                            strokeWidth = 2.dp,
                            modifier = Modifier.padding(2.dp),
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    } else {
                        Text(stringResource(R.string.tickets_composer_submit))
                    }
                }
            }
        }
    }
}

@Composable
private fun LevelOption(label: String, selected: Boolean, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .selectable(selected = selected, onClick = onClick)
            .padding(end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null)
        Text(text = label, style = MaterialTheme.typography.bodyMedium)
    }
}
