# AX8850 AI BOX ID手板图纸（拓竹打印）

本目录提供可直接用于 ID 手板验证的文件：

- `ax8850_ai_box_id_mockup.scad`: 参数化 OpenSCAD 3D 外壳模型
- `ax8850_ai_box_id_drawing.svg`: 2D 尺寸标注图（ID 评审版）

## 使用方式

1. 打开 OpenSCAD，载入 `ax8850_ai_box_id_mockup.scad`
2. 需要导出底壳时，将最后一行改为：
   - `export_base_only();`
3. 需要导出上盖时，将最后一行改为：
   - `export_lid_only();`
4. 执行渲染后导出 STL，用 Bambu Studio 切片打印

## 拓竹打印建议参数（PLA/PETG）

- 层高：0.20 mm
- 壁厚：3-4 道线
- 顶底层：5-6 层
- 填充：20%-30%
- 支撑：仅开孔区域按需开启
- 配合间隙：0.25 mm（已在模型参数中体现）

## 当前版本说明

- 外形尺寸：155 x 78 x 18 mm
- 已包含开孔：RJ45、TF、Reset、LED
- 已包含基础螺柱占位（用于结构评审）
- 当前为 ID 手板版，不是最终可量产模具图

## 下一步建议

- 将实际连接器 3D 模型（STEP）导入后校准开孔位置
- 按真实 PCB 安装孔更新螺柱坐标
- 输出 STEP 给结构工程师做 DFM
