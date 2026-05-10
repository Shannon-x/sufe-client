package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.foundation.layout.Row
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.ui.components.EmptyState
import com.xboard.client.ui.components.LoadingState
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatDateTime
import com.xboard.client.vm.AppViewModel

@Composable
fun NoticesScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val home by viewModel.home.collectAsState()

    LaunchedEffect(Unit) { viewModel.refreshNotices() }

    ScreenScaffold(
        title = stringResource(R.string.notices_title),
        onBack = onBack,
        actions = {
            IconButton(onClick = { viewModel.refreshNotices() }) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.notices_refresh))
            }
        },
    ) { padded ->
        Column(modifier = padded) {
            TopProgressStrip(visible = home.refreshing)
            val notices = home.notices
            when {
                notices == null -> LoadingState(modifier = Modifier.fillMaxSize())
                notices.isEmpty() -> EmptyState(
                    message = stringResource(R.string.notices_empty),
                    modifier = Modifier.fillMaxSize(),
                )
                else -> ScrollableColumn(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    for (notice in notices) {
                        Card(modifier = Modifier.fillMaxWidth()) {
                            Column(
                                modifier = Modifier.padding(16.dp),
                                verticalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Text(
                                    text = notice.title.ifBlank {
                                        stringResource(R.string.notices_untitled)
                                    },
                                    style = MaterialTheme.typography.titleSmall,
                                    fontWeight = FontWeight.SemiBold,
                                )
                                if (notice.tags.isNotEmpty()) {
                                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                        for (tag in notice.tags) {
                                            AssistChip(onClick = {}, label = { Text(tag) })
                                        }
                                    }
                                }
                                Text(
                                    text = notice.content,
                                    style = MaterialTheme.typography.bodyMedium,
                                )
                                notice.createdAt?.let {
                                    Text(
                                        text = formatDateTime(it),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
