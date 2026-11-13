import AppKit
import XCTest
@testable import Sonar

/// Headless scroll benchmark of the real `NSTableView` over 1,000,000
/// synthetic rows (T6.2 AC): no human needed, runs in `xcodebuild test`.
@MainActor
final class ResultsTablePerfTests: XCTestCase {
    func testScrollingOneMillionRowsStaysSmoothAndBounded() async throws {
        let total = 1_000_000
        let pager = ResultsPager()
        let controller = ResultsTableController(pager: pager)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1000, height: 640), styleMask: [.titled, .resizable],
            backing: .buffered, defer: false)
        window.contentView = controller.scrollView
        window.layoutIfNeeded()

        try await pager.reset(loader: SyntheticPageLoader(rowCount: total))
        XCTAssertEqual(controller.tableView.numberOfRows, total)
        controller.scrollView.layoutSubtreeIfNeeded()

        let clock = ContinuousClock()
        var stepMs: [Double] = []
        var maxCached = 0

        func step(to row: Int) async {
            let t0 = clock.now
            controller.tableView.scrollRowToVisible(row)
            controller.scrollView.layoutSubtreeIfNeeded()
            let d = clock.now - t0
            stepMs.append(Double(d.components.seconds) * 1000 + Double(d.components.attoseconds) / 1e15)
            maxCached = max(maxCached, pager.cachedPageCount)
            await Task.yield()  // untimed: let page loads land
            await Task.yield()
        }

        // 1) Sweep the full range in big jumps (fast scroll-bar drag).
        let jump = 667
        var row = 0
        while row < total {
            await step(to: row)
            row += jump
        }
        await step(to: total - 1)
        // 2) Slow scroll: one screen at a time through a deep region.
        row = 400_000
        for _ in 0..<400 {
            await step(to: row)
            row += 25
        }

        let sorted = stepMs.sorted()
        let p50 = sorted[sorted.count / 2]
        let p95 = sorted[Int(Double(sorted.count) * 0.95)]
        let worst = sorted.last ?? 0
        print(String(
            format: "SCROLLBENCH steps=%d p50=%.2fms p95=%.2fms max=%.2fms maxCachedPages=%d/%d",
            sorted.count, p50, p95, worst, maxCached, pager.maxCachedPages))

        XCTAssertLessThan(p95, 16, "p95 scroll step must fit a 60 fps frame")
        XCTAssertLessThanOrEqual(maxCached, pager.maxCachedPages)
        // Rows near the end were reachable (the pager kept up).
        await step(to: total - 1)
        let ok = await waitFor { pager.peek(total - 1) != nil }
        XCTAssertTrue(ok)
        XCTAssertLessThanOrEqual(pager.cachedPageCount, pager.maxCachedPages)
    }
}
