// HTMLRewriter.
package app

import workers.*

/** `/rewrite`: a page rewritten by element and text handlers. */
suspend fun rewrite(): Response {
    val page = Response("<h1>tour</h1><a href=\"/echo\">echo</a><a href=\"/url\">url</a>")
    var links = 0
    // The handlers are a dictionary of callbacks, called once per match.
    val anchors = ElementHandlers().apply {
        setElement { a ->
            links++
            a.setAttribute("href", "https://example.com" + (a.getAttribute("href") ?: ""))
        }
    }
    val heading = ElementHandlers().apply {
        setText { chunk -> if (!chunk.lastInTextNode) chunk.replace(chunk.text.uppercase()) }
    }
    val rewritten = HTMLRewriter().on("a", anchors).on("h1", heading).transform(page).text().await()
    return text("$rewritten\n$links links rewritten\n")
}
