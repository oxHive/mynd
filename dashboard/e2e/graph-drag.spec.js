import { test, expect, createMemory } from './fixtures.js'

// GraphCanvas.vue keys these exact localStorage entries -- see CAMERA_KEY /
// PINNED_KEY in dashboard/src/components/graph/GraphCanvas.vue. Pre-seeding
// them before the app boots (via addInitScript, so it runs before the
// component's module-level `transform = loadCamera()`) pins the node at a
// known world coordinate and zeroes the camera transform, so world space ==
// canvas pixel space -- letting the test click/drag at exact coordinates
// instead of guessing where d3's force simulation happened to settle.
const CAMERA_KEY = 'hivemind.graph.camera'
const PINNED_KEY = 'hivemind.graph.pinned'
const START = { x: 200, y: 200 }

test.describe('graph node drag', () => {
  test('dragging a node persists its new pinned position', async ({ page, api }) => {
    const memory = await createMemory(api, { title: 'draggable node', tags: ['project:e2e'] })

    await page.addInitScript(
      ({ cameraKey, pinnedKey, id, start }) => {
        localStorage.setItem(cameraKey, JSON.stringify({ x: 0, y: 0, k: 1 }))
        localStorage.setItem(pinnedKey, JSON.stringify({ [id]: start }))
      },
      { cameraKey: CAMERA_KEY, pinnedKey: PINNED_KEY, id: memory.id, start: START }
    )

    await page.goto('/#/graph')
    const canvas = page.locator('canvas')
    await expect(canvas).toBeVisible()
    const box = await canvas.boundingBox()

    // Node radius for a lone, edgeless node is nodeRadius's floor (10px) --
    // clicking dead-center of the pinned world coordinate always lands
    // inside the hex regardless of degree.
    const startScreen = { x: box.x + START.x, y: box.y + START.y }
    const target = { x: START.x + 120, y: START.y + 60 }
    const targetScreen = { x: box.x + target.x, y: box.y + target.y }

    await page.mouse.move(startScreen.x, startScreen.y)
    await page.mouse.down()
    // Multiple intermediate steps: d3-drag needs a real move sequence (not a
    // single teleport) to recognize the gesture as a drag rather than a click.
    await page.mouse.move(startScreen.x + 40, startScreen.y + 20, { steps: 5 })
    await page.mouse.move(targetScreen.x, targetScreen.y, { steps: 5 })
    await page.mouse.up()

    // Distance-based rather than exact-equality: the browser's own
    // clientX/offsetX rounding (subpixel canvas layout, DPI) can shift the
    // landed world coordinate by a pixel or two from the requested screen
    // position -- a few px of slack still proves the drag moved the node to
    // roughly `target`, not that it teleported or silently no-op'd.
    await expect
      .poll(async () => {
        const pinned = await page.evaluate(
          key => JSON.parse(localStorage.getItem(key) || '{}'),
          PINNED_KEY
        )
        const p = pinned[memory.id]
        if (!p) return null
        return Math.hypot(p.x - target.x, p.y - target.y)
      })
      .toBeLessThan(5)
  })
})
