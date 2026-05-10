package com.xboard.client.nav

/**
 * Compose Navigation route constants. Single source of truth so screens
 * never embed string literals — the IDE refactor + spell-checker catch
 * typos.
 *
 * The graph is a mostly-flat list (auth flow + main app + detail
 * pages); a handful of routes are detail pages with one dynamic
 * argument (currently just the ticket id).
 */
object Routes {
    // Auth
    const val LOGIN = "auth/login"
    const val REGISTER = "auth/register"
    const val FORGET = "auth/forget"

    // Main
    const val HOME = "home"
    const val CONNECT = "connect"
    const val PLANS = "plans"
    const val ORDERS = "orders"
    const val TICKETS = "tickets"
    const val NOTICES = "notices"

    // Detail (single dynamic arg)
    const val TICKET_DETAIL_ARG = "ticketId"
    const val TICKET_DETAIL = "tickets/{$TICKET_DETAIL_ARG}"
    fun ticketDetail(id: Long): String = "tickets/$id"
}
