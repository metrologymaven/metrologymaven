# GOM Inspect / ZEISS INSPECT Optical 3D

Prefix `GOM`. Compiled from product knowledge on 2026-11-16. **All rows are `[unverified]`** until confirmed by a user of the software.

| ID | Feature | Notes |
|---|---|---|
| GOM.PRJ.01 | Projects and project templates | |
| GOM.PRJ.02 | Stages: multiple parts or measurements in one project | Basis for trend analysis |
| GOM.PRJ.03 | Element explorer: tree of all elements and their state | |
| GOM.PRJ.04 | Tags, keywords and project metadata | |
| GOM.PRJ.05 | Parametric dependency graph with automatic recompute | Signature capability |
| GOM.PRJ.06 | Undo and redo | |
| GOM.PRJ.07 | Change an input and recompute all dependents (e.g. replace the mesh) | |
| GOM.IMP.01 | Import STL meshes | |
| GOM.IMP.02 | Import PLY, OBJ and other mesh formats | |
| GOM.IMP.03 | Import native measurement project or mesh format | Vendor format |
| GOM.IMP.04 | Import ASCII point clouds | |
| GOM.IMP.05 | Import CAD via STEP and IGES | |
| GOM.IMP.06 | Import native CAD (CATIA, NX, Creo, SolidWorks, Parasolid, JT) | Licensed translators |
| GOM.IMP.07 | Import PMI (semantic GD&T and datums) from CAD | |
| GOM.IMP.08 | Import nominal points from CSV or tables | |
| GOM.IMP.09 | Import further measurements as new stages | |
| GOM.EXP.01 | Export meshes | |
| GOM.EXP.02 | Export points and values as CSV or ASCII | |
| GOM.EXP.03 | Export reports as PDF | |
| GOM.EXP.04 | Export Q-DAS (AQDEF) | |
| GOM.EXP.05 | Export to PiWeb | |
| GOM.EXP.06 | Export tables | |
| GOM.EXP.07 | Export geometric elements as STEP or IGES | |
| GOM.MSH.01 | Polygonize a mesh from points | |
| GOM.MSH.02 | Thin a mesh | |
| GOM.MSH.03 | Smooth a mesh | |
| GOM.MSH.04 | Refine a mesh | |
| GOM.MSH.05 | Fill holes, automatically or interactively | |
| GOM.MSH.06 | Remove spikes and noise | |
| GOM.MSH.07 | Close a mesh into a watertight body | |
| GOM.MSH.08 | Cut and clip a mesh with planes or selections | |
| GOM.MSH.09 | Invert normals | |
| GOM.MSH.10 | Merge meshes | |
| GOM.MSH.11 | Selection tools: patch, curvature, lasso, on surface, through or visible only | |
| GOM.MSH.12 | Delete or isolate selections | |
| GOM.MSH.13 | Repair meshes (self-intersections, non-manifold geometry) | |
| GOM.CAD.01 | CAD body display and face (patch) selection | |
| GOM.CAD.02 | CAD groups and part handling | |
| GOM.CAD.03 | Generate nominal elements from CAD faces | |
| GOM.CAD.04 | Control CAD tessellation quality | |
| GOM.CON.01 | Construct point | |
| GOM.CON.02 | Construct line | |
| GOM.CON.03 | Construct plane | |
| GOM.CON.04 | Construct circle | |
| GOM.CON.05 | Construct slot | |
| GOM.CON.06 | Construct rectangle | |
| GOM.CON.07 | Construct cylinder | |
| GOM.CON.08 | Construct cone | |
| GOM.CON.09 | Construct sphere | |
| GOM.CON.10 | Section curves | |
| GOM.CON.11 | Edge and trim curves | |
| GOM.CON.12 | Surface points | |
| GOM.CON.13 | System planes and coordinate systems | |
| GOM.CON.14 | Construct by intersection | |
| GOM.CON.15 | Construct by projection | |
| GOM.CON.16 | Construct by offset | |
| GOM.CON.17 | Construct symmetry, midpoint and bisector | |
| GOM.CON.18 | Gaussian (least-squares) best-fit | |
| GOM.CON.19 | Chebyshev (minimum-zone) fit | |
| GOM.CON.20 | Inner and outer tangential fits | |
| GOM.CON.21 | Outlier filtering in fits | |
| GOM.CON.22 | Automatic construction from CAD with nominal and actual pairing | |
| GOM.CON.23 | Select fitting points by tolerance band | |
| GOM.ALN.01 | Automatic prealignment | |
| GOM.ALN.02 | Global best-fit alignment | |
| GOM.ALN.03 | Local best-fit on selected regions | |
| GOM.ALN.04 | 3-2-1 alignment | |
| GOM.ALN.05 | RPS alignment | |
| GOM.ALN.06 | Alignment by geometric elements | |
| GOM.ALN.07 | Alignment by datum system (ISO 5459 / ASME) | |
| GOM.ALN.08 | Alignment hierarchy and chained alignments | |
| GOM.ALN.09 | Transformation by a known matrix | |
| GOM.ALN.10 | Constrained best-fit (locked degrees of freedom) | |
| GOM.ALN.11 | Initial alignment by picked point pairs | |
| GOM.SRF.01 | Surface comparison on CAD as a color map | |
| GOM.SRF.02 | Surface comparison on actual data | |
| GOM.SRF.03 | Deviation labels | |
| GOM.SRF.04 | Whisker plots on sections | |
| GOM.SRF.05 | Section comparison | |
| GOM.SRF.06 | Edge and trim-line deviation | |
| GOM.SRF.07 | Wall thickness analysis | |
| GOM.SRF.08 | Material thickness | |
| GOM.SRF.09 | Max and min deviation search | |
| GOM.SRF.10 | Tolerance-banded color legends | |
| GOM.DIM.01 | Point-to-point distance | |
| GOM.DIM.02 | Element-to-element distances | |
| GOM.DIM.03 | Angles | |
| GOM.DIM.04 | Diameter and radius | |
| GOM.DIM.05 | Caliper checks | |
| GOM.DIM.06 | Scalar checks on element properties | |
| GOM.DIM.07 | Checks on sections | |
| GOM.DIM.08 | Min and max checks | |
| GOM.DIM.09 | Point deviations in x, y, z and along the normal | |
| GOM.GDT.01 | Flatness | |
| GOM.GDT.02 | Straightness | |
| GOM.GDT.03 | Roundness | |
| GOM.GDT.04 | Cylindricity | |
| GOM.GDT.05 | Parallelism | |
| GOM.GDT.06 | Perpendicularity | |
| GOM.GDT.07 | Angularity | |
| GOM.GDT.08 | Position | |
| GOM.GDT.09 | Concentricity and coaxiality | |
| GOM.GDT.10 | Symmetry | |
| GOM.GDT.11 | Profile of a line | |
| GOM.GDT.12 | Profile of a surface | |
| GOM.GDT.13 | Circular and total runout | |
| GOM.GDT.14 | Datum systems and datum reference frames | |
| GOM.GDT.15 | Material condition modifiers (MMC, LMC) | |
| GOM.GDT.16 | Create checks automatically from PMI | |
| GOM.GDT.17 | ISO or ASME dialect selection | |
| GOM.SPE.01 | Sheet metal: hole and trim positions | |
| GOM.SPE.02 | Sheet metal: springback | |
| GOM.SPE.03 | Sheet metal: hem | |
| GOM.SPE.04 | Gap and flush | |
| GOM.SPE.05 | Airfoil and turbine blade analysis | |
| GOM.SPE.06 | Pattern and hole-pattern checks | |
| GOM.TRD.01 | Statistics across stages | |
| GOM.TRD.02 | Trend charts | |
| GOM.TRD.03 | Process capability (Cp, Cpk) | |
| GOM.TRD.04 | Part-to-part comparison | |
| GOM.TRD.05 | SPC control limits | |
| GOM.CT.01 | CT volume import | X-Ray edition |
| GOM.CT.02 | Surface determination from volume | |
| GOM.CT.03 | Porosity and defect analysis | |
| GOM.CT.04 | Volume-to-volume comparison | |
| GOM.VIZ.01 | Saved views | |
| GOM.VIZ.02 | Clipping planes | |
| GOM.VIZ.03 | Transparency | |
| GOM.VIZ.04 | Color scale editor | |
| GOM.VIZ.05 | Label layout | |
| GOM.VIZ.06 | Visibility and explorer filters | |
| GOM.VIZ.07 | Screenshots | |
| GOM.VIZ.08 | Turntable and animations | |
| GOM.RPT.01 | Report pages | |
| GOM.RPT.02 | Report templates | |
| GOM.RPT.03 | 3D views in the report that can be restored in the scene | |
| GOM.RPT.04 | Tables in reports | |
| GOM.RPT.05 | Diagrams in reports | |
| GOM.RPT.06 | PDF output | |
| GOM.RPT.07 | Multi-stage reports | |
| GOM.AUT.01 | Python scripting | |
| GOM.AUT.02 | Script recording | |
| GOM.AUT.03 | Scripted elements | |
| GOM.AUT.04 | Scripted checks | |
| GOM.AUT.05 | Apps and app store | |
| GOM.AUT.06 | Kiosk operator interface | |
| GOM.AUT.07 | Batch processing | |
| GOM.AUT.08 | Custom dialogs for scripts | |
| GOM.ACQ.01 | Scanner sensor control | Vendor hardware |
| GOM.ACQ.02 | Sensor calibration | |
| GOM.ACQ.03 | Photogrammetry reference-point acquisition | |
| GOM.ACQ.04 | Automated scanning cells | |
| GOM.COR.01 | Digital image correlation and strain | Separate product family |
| GOM.COR.02 | Motion and deformation analysis | |
