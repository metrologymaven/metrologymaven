# Risk assessment

Document: risk, template version 0.1.0

Interpretation: provisional. The framework references and judgements in this template have not been reviewed by a qualified person; check each one before relying on it.

Pre-filled for intended uses: prismatic\_gdt, scan\_inspection

This template is part of the Metrology Maven Validation Accelerator Pack and is licensed under the Creative Commons Attribution 4.0 International licence (CC-BY-4.0). You may copy, modify and reuse it, including for your own validation documents and commercially, provided you give attribution.

## Framework references

| Designator | Edition | Clause |
| --- | --- | --- |
| FDA guidance: Computer Software Assurance | 2022 draft \[unverified\] | V \[unverified\] |
| ICH Q9(R1) | 2023 | 4 \[unverified\] |
| EU GMP Annex 11 | 2011 | 1 |
| ISPE GAMP 5 | 2nd (2022) | 5 \[unverified\] |

## Context

This assessment traces each intended use to the features it relies on and to the risk to your process if a feature fails. It then chooses how much testing each feature needs. You own the assessment; the proposed failure modes are a starting point.

**System name and version:** (to be completed)

**Process the system supports:** (to be completed)

**User requirements specification (document ID and version):** (to be completed)

## Method

For each row, decide whether a failure of the feature would be a high process risk, meaning it could compromise product quality or patient safety without being caught. High process risk calls for scripted testing with recorded evidence. Other rows can use unscripted or ad hoc testing, recorded briefly. Write down the reason for every decision.

**Default assurance approach for rows that are not high process risk:** (choose one) (options: unscripted testing / ad hoc testing / scripted testing)

## Risk register

**Intended use, feature and process risk**

| ID | Intended use | Feature | What could go wrong | Process risk (high or not high) | Assurance activity | Rationale | Pre-filled for |
| --- | --- | --- | --- | --- | --- | --- | --- |
| RA-TR-01 | Transcription reports | Manual entry of transcribed values |  |  |  |  |  |
| RA-TR-02 | Transcription reports | Report generation |  |  |  |  |  |
| RA-GD-01 | Prismatic GD\&T subset | Tolerance evaluation | A tolerance is evaluated under an interpretation you did not intend, and the result looks settled. |  |  |  | prismatic\_gdt |
| RA-GD-02 | Prismatic GD\&T subset | Datum reference frames | A datum feature is fitted with the wrong method and every dependent result shifts. |  |  |  | prismatic\_gdt |
| RA-SC-01 | Scan inspection | Alignment | Deviations are computed in a wrong or out-of-date alignment. |  |  |  | scan\_inspection |
| RA-SC-02 | Scan inspection | Point filtering and smoothing | Filtering removes or moves points that show a real defect. |  |  |  | scan\_inspection |
| RA-CU-01 | Your intended use | Your feature |  |  |  |  |  |

## Conclusion

**Summary of the testing chosen:** (to be completed)

**Author:** name \_\_\_\_\_\_, signature \_\_\_\_\_\_, date \_\_\_\_\_\_

**Approver (quality):** name \_\_\_\_\_\_, signature \_\_\_\_\_\_, date \_\_\_\_\_\_
