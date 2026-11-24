import SwiftUI

/// One tab: a Liquid Glass capsule, tinted when it is in front.
struct TabChip: View {
    let tab: WorkbenchTab
    let isSelected: Bool
    var fillsWidth = false
    /// A width shared out by the tab row, so tabs narrow before they scroll.
    var width: CGFloat?
    let select: () -> Void
    let close: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // The icon and the title are one button to VoiceOver, as tall as
            // the chip, so the whole chip is a target.
            HStack(spacing: 6) {
                Image(systemName: tab.systemImage)
                    .foregroundStyle(isSelected ? Color.accentColor : .secondary)
                    .accessibilityHidden(true)
                Text(tab.title)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if fillsWidth {
                    Spacer(minLength: 0)
                }
            }
            .padding(.vertical, Self.verticalPadding)
            .contentShape(Rectangle())
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
            .accessibilityAction(.default) { select() }
            if tab.isClosable {
                Button(action: close) {
                    Image(systemName: "xmark")
                        .font(.caption2.weight(.bold))
                        .foregroundStyle(.secondary)
                        .touchTarget(width: 32, height: 32)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text("Close tab"))
                // The target reaches into the chip's padding.
                #if os(iOS)
                .padding(.horizontal, -10)
                #endif
            }
        }
        .font(.callout.weight(isSelected ? .semibold : .regular))
        .padding(.horizontal, 12)
        .frame(maxWidth: fillsWidth ? .infinity : min(width ?? 220, 220), alignment: .leading)
        .contentShape(Capsule())
        .onTapGesture(perform: select)
        .glassEffect(
            isSelected ? .regular.tint(Color.accentColor.opacity(0.25)).interactive() : .regular.interactive(),
            in: .capsule
        )
        .contextMenu { TabMenu(tab: tab) }
    }

    /// Space above and below a tab's title: a little more on iOS, so a tab
    /// is tall enough to touch.
    #if os(iOS)
    private static let verticalPadding: CGFloat = 10
    #else
    private static let verticalPadding: CGFloat = 7
    #endif
}

/// What a tab offers beside selecting and closing it.
private struct TabMenu: View {
    let tab: WorkbenchTab
    @Environment(TabsModel.self) private var tabs

    var body: some View {
        if tabs.sideID == tab.id {
            Button("Close Split", systemImage: "rectangle") { tabs.sideID = nil }
        } else {
            Button("Open to the Side", systemImage: "rectangle.split.2x1") { tabs.openToTheSide(tab) }
        }
        if tab.isClosable {
            Button("Close Tab", systemImage: "xmark") { tabs.close(tab) }
        }
    }
}

/// Tabs in a row above the documents.
struct HorizontalTabStrip: View {
    @Environment(TabsModel.self) private var tabs
    @State private var rowWidth: CGFloat = 0

    /// Each tab's width: an equal share of the row, as in a browser, between
    /// a readable minimum and the usual maximum; past that the row scrolls.
    private var tabWidth: CGFloat? {
        guard rowWidth > 0 else { return nil }
        let count = CGFloat(max(tabs.tabs.count, 1))
        return min(220, max(120, (rowWidth - 24 - 8 * (count - 1)) / count))
    }

    var body: some View {
        ScrollViewReader { reader in
            ScrollView(.horizontal, showsIndicators: false) {
                GlassEffectContainer(spacing: 8) {
                    HStack(spacing: 8) {
                        ForEach(tabs.tabs) { tab in
                            TabChip(
                                tab: tab, isSelected: tab.id == tabs.selectedID, width: tabWidth,
                                select: { tabs.selectedID = tab.id }, close: { tabs.close(tab) })
                            .id(tab.id)
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                }
            }
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { rowWidth = $0 }
            .onChange(of: tabs.selectedID, initial: true) { _, id in
                withAnimation { reader.scrollTo(id) }
            }
        }
        .animation(.smooth, value: tabs.tabs)
    }
}

/// Tabs in a column beside the documents, Edge style.
struct VerticalTabStrip: View {
    @Environment(TabsModel.self) private var tabs

    var body: some View {
        ScrollView {
            GlassEffectContainer(spacing: 6) {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(tabs.tabs) { tab in
                        TabChip(
                            tab: tab, isSelected: tab.id == tabs.selectedID, fillsWidth: true,
                            select: { tabs.selectedID = tab.id }, close: { tabs.close(tab) })
                    }
                }
                .padding(10)
            }
        }
        .animation(.smooth, value: tabs.tabs)
    }
}
