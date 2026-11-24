package io.felixbennett.paperloom.share

import android.os.Build
import android.view.View
import android.view.WindowInsets
import android.webkit.WebView

/**
 * Keeps the app clear of the status bar, the navigation or gesture bar,
 * display cutouts and the keyboard. The activity draws edge to edge, and the
 * WebView does not report these as CSS safe areas, so the page's own
 * controls ended up under the clock and the gesture bar.
 */
object SystemBars {
    // The pre-Android 11 branch can only use the deprecated inset getters.
    @Suppress("DEPRECATION")
    fun keepClear(webView: WebView) {
        webView.post {
            val holder = webView.parent as? View ?: return@post
            holder.setOnApplyWindowInsetsListener { view, insets ->
                if (Build.VERSION.SDK_INT >= 30) {
                    val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                    val keyboard = insets.getInsets(WindowInsets.Type.ime())
                    view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard.bottom))
                    // Taken here, so the page does not add them a second time.
                    WindowInsets.CONSUMED
                } else {
                    view.setPadding(
                        insets.systemWindowInsetLeft,
                        insets.systemWindowInsetTop,
                        insets.systemWindowInsetRight,
                        insets.systemWindowInsetBottom,
                    )
                    insets.consumeSystemWindowInsets()
                }
            }
            holder.requestApplyInsets()
        }
    }
}
