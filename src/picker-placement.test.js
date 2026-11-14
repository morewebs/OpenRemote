import assert from 'node:assert/strict'
import test from 'node:test'
import * as placement from './picker-anchor.js'

test('a menu above the mobile composer leaves an eight pixel gap', () => {
  const result = placement.fitPicker({ left: 16, top: 700 }, { width: 340, height: 300 }, { width: 412, height: 844 }, 'above')
  assert.deepEqual(result, { left: 16, top: 700, maxHeight: 828 })
  assert.equal(result.top - 300 - 8, 392)
})

test('menus clamp both edges on a narrow or keyboard-reduced viewport', () => {
  for (const spot of [{left:-40,top:-10},{left:400,top:800}]) {
    for (const place of ['above','below']) {
      const size = { width: 300, height: 250 }
      const viewport = {width:320,height:300,offsetTop:40}
      const result = placement.fitPicker(spot,size,viewport,place)
      const visualTop = result.top - (place === 'above' ? size.height + 8 : 0)
      assert.ok(result.left >= 8 && result.left + size.width <= 312)
      assert.ok(visualTop >= 48 && visualTop + size.height <= 332)
    }
  }
})

test('placement always starts from the anchor, so rerenders cannot move the menu', () => {
  const spot = {left:16,top:720}
  const size = {width:340,height:280}
  const viewport = {width:412,height:844}
  const first = placement.fitPicker(spot,size,viewport,'below')
  assert.equal(first.top,556)
  for(let i=0;i<10;i++) assert.deepEqual(placement.fitPicker(spot,size,viewport,'below'),first)
})
