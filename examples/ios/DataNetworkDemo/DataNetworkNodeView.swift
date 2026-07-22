import SwiftUI

private let executionRpc = "https://story-rpc.publicnode.com"
private let consensusRpc = "https://story-consensus-rpc.publicnode.com"
private let trustHeight: UInt64 = 20_044_541
private let trustHash = "9929C08444D99A82908E007CB0A45AF073A424630E46A1289E6C7C2CB98C8449"

private enum Palette {
    static let base = Color(red: 0.102, green: 0.102, blue: 0.102)
    static let card = Color(red: 0.188, green: 0.188, blue: 0.188)
    static let border = Color(red: 0.188, green: 0.188, blue: 0.188)
    static let distinct = Color(red: 0.310, green: 0.310, blue: 0.310)
    static let primary = Color(red: 0.973, green: 0.973, blue: 0.965)
    static let secondary = Color(red: 0.725, green: 0.725, blue: 0.702)
    static let tertiary = Color(red: 0.502, green: 0.502, blue: 0.502)
    static let accent = Color(red: 0.965, green: 1.000, blue: 0.000)
    static let positive = Color(red: 0.357, green: 0.667, blue: 0.498)
    static let negative = Color(red: 0.910, green: 0.384, blue: 0.341)
}

private enum ClientPhase {
    case syncing
    case live
    case reconnecting
    case error

    var label: String {
        switch self {
        case .syncing: "Syncing"
        case .live: "Live"
        case .reconnecting: "Reconnecting"
        case .error: "Error"
        }
    }

    var color: Color {
        switch self {
        case .syncing: Palette.tertiary
        case .live: Palette.positive
        case .reconnecting: Palette.accent
        case .error: Palette.negative
        }
    }
}

private struct VerifiedBlock: Identifiable {
    let number: UInt64
    let hash: String
    let timestamp: UInt64
    let transactionCount: Int

    var id: UInt64 { number }

    init(json: String) throws {
        let data = Data(json.utf8)
        guard let block = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw BlockDecodingError.invalidBlock
        }
        guard let number = Self.quantity(block["number"]) else {
            throw BlockDecodingError.missingField("number")
        }
        guard let hash = block["hash"] as? String else {
            throw BlockDecodingError.missingField("hash")
        }
        guard let timestamp = Self.quantity(block["timestamp"]) else {
            throw BlockDecodingError.missingField("timestamp")
        }

        self.number = number
        self.hash = hash
        self.timestamp = timestamp
        self.transactionCount = (block["transactions"] as? [Any])?.count ?? 0
    }

    private static func quantity(_ value: Any?) -> UInt64? {
        if let number = value as? NSNumber {
            return number.uint64Value
        }
        guard let string = value as? String else {
            return nil
        }
        if string.hasPrefix("0x") {
            return UInt64(string.dropFirst(2), radix: 16)
        }
        return UInt64(string)
    }
}

private enum BlockDecodingError: LocalizedError {
    case invalidBlock
    case missingField(String)

    var errorDescription: String? {
        switch self {
        case .invalidBlock: "The light client returned invalid block JSON."
        case .missingField(let field): "The verified block is missing \(field)."
        }
    }
}

@MainActor
private final class DataNetworkViewModel: ObservableObject {
    @Published var error: Error?
    @Published var isStarting = false
    @Published var isRunning = false
    @Published var isSynced = false
    @Published var latestHeight: UInt64?
    @Published var blocks: [VerifiedBlock] = []

    private var node: DataNetworkNode?
    private var statsTimer: Timer?
    private var lastSeen: UInt64?

    var phase: ClientPhase {
        if error != nil { return isSynced ? .reconnecting : .error }
        return isSynced ? .live : .syncing
    }

    deinit {
        statsTimer?.invalidate()
    }

    func startNode() async {
        guard !isStarting, !isRunning else { return }
        isStarting = true
        error = nil

        let paths = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)
        let config = NodeConfig(
            basePath: paths[0].path,
            network: .mainnet,
            executionRpc: executionRpc,
            verifiableApi: nil,
            consensusRpc: consensusRpc,
            trustHeight: trustHeight,
            trustHash: trustHash
        )

        do {
            let node = try DataNetworkNode(config: config)
            self.node = node
            _ = try await node.start()
            isRunning = await node.isRunning()
            try await node.waitSynced()
            isSynced = true
            isStarting = false
            await updateStats()
            statsTimer = pollStats()
        } catch {
            isStarting = false
            self.error = error
        }
    }

    func stopNode() async {
        statsTimer?.invalidate()
        statsTimer = nil
        isStarting = false
        isRunning = false
        isSynced = false
        latestHeight = nil
        blocks = []
        lastSeen = nil

        do {
            try await node?.stop()
            node = nil
        } catch {
            self.error = error
        }
    }

    private func updateStats() async {
        guard let node, isSynced else { return }

        do {
            let heightHex = try await node.getBlockNumber()
            guard let height = Self.quantity(heightHex), height != lastSeen else { return }

            let json = try await node.getBlockByNumber(block: heightHex, fullTx: true)
            let block = try VerifiedBlock(json: json)
            lastSeen = height
            latestHeight = block.number
            blocks.removeAll { $0.number == block.number }
            blocks.insert(block, at: 0)
            blocks = Array(blocks.prefix(10))
            error = nil
        } catch {
            self.error = error
        }
    }

    private func pollStats() -> Timer {
        Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.updateStats()
            }
        }
    }

    private static func quantity(_ value: String) -> UInt64? {
        if value.hasPrefix("0x") {
            return UInt64(value.dropFirst(2), radix: 16)
        }
        return UInt64(value)
    }
}

struct DataNetworkNodeView: View {
    @StateObject private var viewModel = DataNetworkViewModel()

    var body: some View {
        ZStack(alignment: .top) {
            Palette.base.ignoresSafeArea()

            ScrollView {
                VStack(spacing: 24) {
                    HeaderView(phase: viewModel.phase, latestHeight: viewModel.latestHeight)
                        .padding(.bottom, 24)

                    BlocksCard(
                        phase: viewModel.phase,
                        latestHeight: viewModel.latestHeight,
                        blocks: viewModel.blocks,
                        error: viewModel.error
                    )

                    DetailsView(
                        phase: viewModel.phase,
                        latestHeight: viewModel.latestHeight
                    )
                }
                .padding(.horizontal, 16)
                .padding(.top, 28)
                .padding(.bottom, 48)
            }

            if viewModel.phase == .syncing {
                ProgressView()
                    .progressViewStyle(.linear)
                    .tint(Palette.accent)
            }
        }
        .task {
            await viewModel.startNode()
        }
    }
}

private struct HeaderView: View {
    let phase: ClientPhase
    let latestHeight: UInt64?

    var body: some View {
        HStack(spacing: 8) {
            Image("DataSymbol")
                .resizable()
                .scaledToFit()
                .frame(width: 24, height: 24)
                .accessibilityHidden(true)

            Text("DATA")
                .font(.system(size: 21, weight: .medium))
                .foregroundStyle(Palette.primary)

            StatusBadge(label: "Mainnet", phase: phase)

            Spacer(minLength: 12)

            Text(latestHeight.map(String.init) ?? "...")
                .font(.system(size: 13, design: .monospaced))
                .foregroundStyle(Palette.secondary)
        }
    }
}

private struct StatusBadge: View {
    let label: String
    let phase: ClientPhase

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(phase.color)
                .frame(width: 6, height: 6)
            Text(label)
                .font(.system(size: 13))
                .foregroundStyle(Palette.secondary)
        }
        .padding(.horizontal, 9)
        .frame(height: 28)
        .background(Palette.card)
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(Palette.distinct, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }
}

private struct BlocksCard: View {
    let phase: ClientPhase
    let latestHeight: UInt64?
    let blocks: [VerifiedBlock]
    let error: Error?

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                HStack(spacing: 8) {
                    Text("Blocks")
                    if let latestHeight {
                        Text("(\(latestHeight))")
                            .foregroundStyle(Palette.tertiary)
                    }
                }
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(Palette.primary)
                .textCase(.uppercase)
                .padding(.horizontal, 18)
                .frame(height: 36)
                .overlay(alignment: .bottom) {
                    Rectangle()
                        .fill(Palette.accent)
                        .frame(height: 2)
                        .padding(.horizontal, 16)
                }

                Spacer()

                HStack(spacing: 6) {
                    Circle()
                        .fill(phase.color)
                        .frame(width: 5, height: 5)
                    Text(phase.label)
                }
                .font(.system(size: 11))
                .foregroundStyle(phase.color)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(phase.color.opacity(0.10))
                .padding(.trailing, 18)
            }

            ScrollView(.horizontal, showsIndicators: true) {
                VStack(spacing: 0) {
                    BlockHeaderRow()
                    DashedDivider()

                    if error != nil {
                        Color.clear
                            .frame(width: 680, height: 245)
                    } else if blocks.isEmpty {
                        ForEach(0..<5, id: \.self) { index in
                            SkeletonRow()
                            if index < 4 { DashedDivider() }
                        }
                    } else {
                        ForEach(Array(blocks.enumerated()), id: \.element.id) { index, block in
                            BlockRow(block: block, isNewest: index == 0)
                            if index < blocks.count - 1 { DashedDivider() }
                        }
                    }
                }
                .frame(minWidth: 680)
                .background(Palette.card)
            }
            .overlay(alignment: .bottom) {
                if error != nil {
                    Text(
                        phase == .reconnecting
                            ? "Connection interrupted. Retrying..."
                            : "Unable to connect to DATA Network."
                    )
                        .font(.system(size: 13, design: .monospaced))
                        .foregroundStyle(Palette.negative)
                        .multilineTextAlignment(.center)
                        .lineLimit(5)
                        .truncationMode(.middle)
                        .padding(.horizontal, 20)
                        .frame(maxWidth: .infinity, minHeight: 245)
                        .background(Palette.card)
                }
            }
            .overlay(alignment: .top) {
                Rectangle().fill(Palette.border).frame(height: 1)
            }
        }
        .background(Palette.base)
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(Palette.border, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }
}

private struct BlockHeaderRow: View {
    var body: some View {
        HStack(spacing: 0) {
            GridText("Block", width: 110)
            GridText("Hash", width: 330)
            GridText("Time", width: 150, alignment: .trailing)
            GridText("Txns", width: 90, alignment: .trailing)
        }
        .frame(height: 36)
        .foregroundStyle(Palette.tertiary)
    }
}

private struct BlockRow: View {
    let block: VerifiedBlock
    let isNewest: Bool

    @State private var highlighted = true

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.timeStyle = .medium
        return formatter
    }()

    var body: some View {
        HStack(spacing: 0) {
            GridText("#\(block.number)", width: 110, color: Palette.accent, weight: .medium)
            GridText(block.hash, width: 330)
            GridText(
                Self.timeFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(block.timestamp))),
                width: 150,
                alignment: .trailing,
                color: Palette.secondary
            )
            GridText(String(block.transactionCount), width: 90, alignment: .trailing, color: Palette.secondary)
        }
        .frame(height: 48)
        .background(highlighted && isNewest ? Palette.positive.opacity(0.16) : Color.clear)
        .onAppear {
            withAnimation(.easeOut(duration: 0.5)) {
                highlighted = false
            }
        }
    }
}

private struct GridText: View {
    let value: String
    let width: CGFloat
    let alignment: Alignment
    let color: Color
    let weight: Font.Weight

    init(
        _ value: String,
        width: CGFloat,
        alignment: Alignment = .leading,
        color: Color = Palette.primary,
        weight: Font.Weight = .regular
    ) {
        self.value = value
        self.width = width
        self.alignment = alignment
        self.color = color
        self.weight = weight
    }

    var body: some View {
        Text(value)
            .font(.system(size: 13, weight: weight, design: .monospaced))
            .foregroundStyle(color)
            .lineLimit(1)
            .truncationMode(.middle)
            .padding(.horizontal, 10)
            .frame(width: width, alignment: alignment)
    }
}

private struct SkeletonRow: View {
    var body: some View {
        HStack(spacing: 0) {
            SkeletonCell(width: 110, barWidth: 80)
            SkeletonCell(width: 330, barWidth: 180)
            SkeletonCell(width: 150, barWidth: 80, alignment: .trailing)
            SkeletonCell(width: 90, barWidth: 24, alignment: .trailing)
        }
        .frame(height: 48)
    }
}

private struct SkeletonCell: View {
    let width: CGFloat
    let barWidth: CGFloat
    var alignment: Alignment = .leading

    var body: some View {
        RoundedRectangle(cornerRadius: 4)
            .fill(Palette.distinct.opacity(0.7))
            .frame(width: barWidth, height: 12)
            .padding(.horizontal, 10)
            .frame(width: width, alignment: alignment)
    }
}

private struct DetailsView: View {
    let phase: ClientPhase
    let latestHeight: UInt64?

    var body: some View {
        VStack(spacing: 14) {
            InfoCard(title: "Light client") {
                InfoRow(label: "Status", value: phase.label, valueColor: phase.color)
                DashedDivider()
                InfoRow(label: "Latest verified", value: latestHeight.map(String.init) ?? "...")
                DashedDivider()
                InfoRow(label: "Chain ID", value: "1514")
                DashedDivider()
                InfoRow(label: "Storage", value: "App cache")
            }

            InfoCard(title: "Trust") {
                InfoRow(label: "Height", value: String(trustHeight))
                DashedDivider()
                InfoRow(label: "Hash", value: trustHash)
                DashedDivider()
                InfoRow(label: "Consensus RPC", value: consensusRpc)
                DashedDivider()
                InfoRow(label: "Execution RPC", value: executionRpc)
            }
        }
    }
}

private struct InfoCard<Content: View>: View {
    let title: String
    let content: Content

    init(title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(spacing: 0) {
            Text(title)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(Palette.tertiary)
                .textCase(.uppercase)
                .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
                .padding(.horizontal, 18)

            VStack(spacing: 0) {
                content
            }
            .background(Palette.card)
            .overlay(alignment: .top) {
                Rectangle().fill(Palette.border).frame(height: 1)
            }
        }
        .background(Palette.base)
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .stroke(Palette.border, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }
}

private struct InfoRow: View {
    let label: String
    let value: String
    var valueColor: Color = Palette.primary

    var body: some View {
        HStack(spacing: 16) {
            Text(label)
                .font(.system(size: 13))
                .foregroundStyle(Palette.tertiary)
                .fixedSize()

            Spacer(minLength: 0)

            Text(value)
                .font(.system(size: 13, design: .monospaced))
                .foregroundStyle(valueColor)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .padding(.horizontal, 18)
        .frame(minHeight: 48)
    }
}

private struct DashedDivider: View {
    var body: some View {
        GeometryReader { geometry in
            Path { path in
                path.move(to: .zero)
                path.addLine(to: CGPoint(x: geometry.size.width, y: 0))
            }
            .stroke(Palette.distinct, style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
        }
        .frame(height: 1)
    }
}
