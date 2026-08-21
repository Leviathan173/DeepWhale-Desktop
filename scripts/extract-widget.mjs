import fs from 'node:fs'
const src = fs.readFileSync('lib/index.js', 'utf8')
const start = src.indexOf('const WIDGET_JS = `')
if (start < 0) { console.error('marker not found'); process.exit(1) }
const open = src.indexOf('`', start) // opening backtick
const contentStart = open + 1
const close = src.indexOf('`', contentStart)
if (close < 0) { console.error('closing backtick not found'); process.exit(1) }
const out = src.slice(contentStart, close) + '\n'
fs.writeFileSync('desktop/public/widget.js', out)
console.log('extracted', out.split('\n').length, 'lines -> desktop/public/widget.js')