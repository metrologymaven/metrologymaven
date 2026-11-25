# Units and quantities

The kernel computes in SI metres (`m`) and keeps authored decimals with their unit.
Command names such as demo.read_item_two keep literal underscores.

| Quantity | Canonical unit | Example |
| --- | :---: | ---: |
| Length | `m` | 0.025 |
| Angle | `rad` | 1.5708 |

## Command example

```sh
fid demo read-item --unit mm
```

See also [Introduction](index.md).
