import { onBeforeUnmount, onMounted, type Ref } from 'vue'

/** Minimum comfortable width before another settings column is allowed. */
const MIN_COLUMN_WIDTH = 520

/**
 * Packs complete settings groups vertically before opening another column.
 * @param heights Outer heights of the groups, in reading order.
 * @param limit Available height per column.
 * @param gap Vertical spacing between groups.
 * @returns Required columns and the tallest occupied column.
 */
function packGroups(heights: number[], limit: number, gap: number) {
  let columns = 1
  let currentHeight = 0
  let height = 0
  for (const groupHeight of heights) {
    if (currentHeight > 0 && currentHeight + gap + groupHeight > limit) {
      height = Math.max(height, currentHeight)
      columns += 1
      currentHeight = 0
    }
    currentHeight += (currentHeight > 0 ? gap : 0) + groupHeight
  }
  return { columns, height: Math.max(height, currentHeight) }
}

/**
 * Uses the dialog's available height first, then columns, then vertical scrolling.
 * @param container Settings container whose CSS layout variables are owned by this composable.
 * @param onWidthChange Receives the preferred dialog width, including its horizontal padding.
 */
export function useParametersLayout(
  container: Readonly<Ref<HTMLElement | null>>,
  onWidthChange: (width: number) => void,
) {
  let observer: ResizeObserver | null = null
  let frame = 0
  let signature = ''

  /** Measures available width independently of the dialog's current fitted width. */
  function widthMetrics(root: HTMLElement) {
    const body = root.closest<HTMLElement>('.v-card-text')
    const style = body ? getComputedStyle(body) : null
    const padding = style ? parseFloat(style.paddingLeft) + parseFloat(style.paddingRight) : 0
    const scrollbar = body ? body.offsetWidth - body.clientWidth : 0
    const chrome = padding + scrollbar
    return { available: Math.max(0, window.innerWidth - 32 - chrome), chrome }
  }

  /** Measures room below the title and above the dialog actions. */
  function availableHeight(root: HTMLElement): number {
    const body = root.closest<HTMLElement>('.v-card-text')
    const card = root.closest<HTMLElement>('.parameters-dialog__surface')
    const style = body ? getComputedStyle(body) : null
    const padding = style ? parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) : 0
    const header = card?.querySelector<HTMLElement>('.v-card-item')?.offsetHeight ?? 0
    const actions = card?.querySelector<HTMLElement>('.parameters-dialog__actions')?.offsetHeight ?? 0
    const messages = body ? [...body.children]
      .filter((element) => !element.contains(root))
      .reduce((height, element) => height + (element as HTMLElement).offsetHeight, 0) : 0
    return Math.max(0, window.innerHeight - 32 - header - actions - padding - messages)
  }

  /** Captures final geometry so layout-owned changes do not retrigger measurement. */
  function geometry(root: HTMLElement): string {
    return [root.clientWidth, widthMetrics(root).available, availableHeight(root), ...[...root.children].map((group) => {
      const element = group as HTMLElement
      return `${element.offsetWidth}:${element.offsetHeight}`
    })].join('|')
  }

  /** Applies the occupied height and fits the surrounding dialog to its columns.
   * @param root Settings container being arranged.
   * @param height Occupied height of the tallest column.
   * @param columns Number of occupied columns.
   * @param columnWidth Actual group width at this layout pass.
   * @param gap Horizontal spacing between columns.
   */
  function finish(root: HTMLElement, height: number, columns: number, columnWidth: number, gap: number): void {
    root.style.setProperty('--parameters-layout-height', `${height + 1}px`)
    onWidthChange(columns * columnWidth + (columns - 1) * gap + widthMetrics(root).chrome)
    signature = geometry(root)
  }

  /** Finds the smallest number of columns that fits, without fragmenting a group. */
  function arrange(): void {
    frame = 0
    const root = container.value
    if (!root || root.clientWidth === 0) return
    const groups = [...root.children] as HTMLElement[]
    const gap = parseFloat(getComputedStyle(root).rowGap) || 0
    const limit = availableHeight(root)
    const availableWidth = widthMetrics(root).available
    const maximumGroupWidth = parseFloat(getComputedStyle(groups[0]!).maxWidth)
    const maximumColumns = Math.max(1, Math.min(groups.length, Math.floor((availableWidth + gap) / (MIN_COLUMN_WIDTH + gap))))
    let heights: number[] = []
    let columnWidth = 0
    root.style.setProperty('--parameters-layout-height', 'auto')

    for (let columns = 1; columns <= maximumColumns; columns += 1) {
      columnWidth = Math.min(maximumGroupWidth, (availableWidth - gap * (columns - 1)) / columns)
      root.style.setProperty('--parameters-column-width', `${columnWidth}px`)
      heights = groups.map((group) => {
        const style = getComputedStyle(group)
        return group.offsetHeight + 1 + parseFloat(style.marginTop) + parseFloat(style.marginBottom)
      })
      const packed = packGroups(heights, limit, gap)
      if (Math.max(...heights) <= limit && packed.columns <= columns) {
        finish(root, packed.height, packed.columns, columnWidth, gap)
        return
      }
    }

    // Once all usable columns are occupied, increase their height for vertical scrolling.
    let lower = Math.max(...heights, 0)
    let upper = heights.reduce((sum, height) => sum + height + gap, 0)
    for (let pass = 0; pass < 16; pass += 1) {
      const middle = (lower + upper) / 2
      if (packGroups(heights, middle, gap).columns <= maximumColumns) upper = middle
      else lower = middle
    }
    finish(root, upper, packGroups(heights, upper, gap).columns, columnWidth, gap)
  }

  /** Queues one measurement after an external geometry change. */
  function schedule(): void {
    if (!frame && container.value && geometry(container.value) !== signature) {
      frame = requestAnimationFrame(arrange)
    }
  }

  onMounted(() => {
    const root = container.value
    if (!root) return
    observer = new ResizeObserver(schedule)
    for (const element of [root, ...root.children, root.closest('.v-card-text'), root.closest('.parameters-dialog__surface')]) {
      if (element) observer.observe(element)
    }
    window.addEventListener('resize', schedule)
    schedule()
  })

  onBeforeUnmount(() => {
    observer?.disconnect()
    cancelAnimationFrame(frame)
    window.removeEventListener('resize', schedule)
  })
}
