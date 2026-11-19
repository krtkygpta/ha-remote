package dev.cmf.ha

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Restarts the watch connection after a phone reboot or an app update, so the link to the
 * watch resumes without opening the app. Does nothing when no watch was configured yet.
 */
class BootReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val address = context.getSharedPreferences("remote", Context.MODE_PRIVATE)
            .getString("watch", null) ?: return
        val service = Intent(context, CompanionService::class.java)
            .setAction(CompanionService.ACTION_START)
            .putExtra(CompanionService.EXTRA_ADDRESS, address)
        runCatching { context.startForegroundService(service) }
    }
}
