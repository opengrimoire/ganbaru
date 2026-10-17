const CUSTOM_FIELD_OPTION_CONNECTOR_SPINE_OFFSET_REM = 1.125;
const CUSTOM_FIELD_OPTION_CONNECTOR_START_Y_REM = -0.625;
const CUSTOM_FIELD_OPTION_CONNECTOR_FIRST_CENTER_Y_REM = 1;
const CUSTOM_FIELD_OPTION_CONNECTOR_ROW_STEP_REM = 2.5;
const CUSTOM_FIELD_OPTION_CONNECTOR_RADIUS_REM = 0.45;
const CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM = 0.15;

function svgNumber(value: number): string {
  return Number(value.toFixed(3)).toString();
}

function customFieldOptionConnectorCenterY(index: number): number {
  return CUSTOM_FIELD_OPTION_CONNECTOR_FIRST_CENTER_Y_REM
    + index * CUSTOM_FIELD_OPTION_CONNECTOR_ROW_STEP_REM;
}

function customFieldOptionConnectorTop(): number {
  return CUSTOM_FIELD_OPTION_CONNECTOR_START_Y_REM
    - CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM;
}

function customFieldOptionConnectorHeight(optionCount: number): number {
  const bottom = customFieldOptionConnectorCenterY(optionCount - 1)
    + CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM;
  return bottom - customFieldOptionConnectorTop();
}

export function customFieldOptionConnectorStyle(optionCount: number): string {
  const width = CUSTOM_FIELD_OPTION_CONNECTOR_SPINE_OFFSET_REM
    + CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM * 2;
  const left = -(CUSTOM_FIELD_OPTION_CONNECTOR_SPINE_OFFSET_REM + CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM);
  return [
    `left: ${svgNumber(left)}rem`,
    `top: ${svgNumber(customFieldOptionConnectorTop())}rem`,
    `width: ${svgNumber(width)}rem`,
    `height: ${svgNumber(customFieldOptionConnectorHeight(optionCount))}rem`,
  ].join("; ");
}

export function customFieldOptionConnectorViewBox(optionCount: number): string {
  const left = -CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM;
  const width = CUSTOM_FIELD_OPTION_CONNECTOR_SPINE_OFFSET_REM
    + CUSTOM_FIELD_OPTION_CONNECTOR_PADDING_REM * 2;
  return [
    svgNumber(left),
    svgNumber(customFieldOptionConnectorTop()),
    svgNumber(width),
    svgNumber(customFieldOptionConnectorHeight(optionCount)),
  ].join(" ");
}

export function customFieldOptionConnectorPath(optionCount: number): string {
  if (optionCount <= 0) return "";
  const branchRadius = CUSTOM_FIELD_OPTION_CONNECTOR_RADIUS_REM;
  const spineEndY = customFieldOptionConnectorCenterY(optionCount - 1) - branchRadius;
  const parts = [
    `M 0 ${svgNumber(CUSTOM_FIELD_OPTION_CONNECTOR_START_Y_REM)}`,
    `L 0 ${svgNumber(spineEndY)}`,
  ];
  for (let index = 0; index < optionCount; index += 1) {
    const centerY = customFieldOptionConnectorCenterY(index);
    const curveStartY = centerY - branchRadius;
    const curveMidX = branchRadius * 0.32;
    const curveMidY = centerY - branchRadius * 0.22;
    parts.push(
      `M 0 ${svgNumber(curveStartY)}`,
      `C 0 ${svgNumber(curveMidY)} ${svgNumber(curveMidX)} ${svgNumber(centerY)} ${svgNumber(branchRadius)} ${svgNumber(centerY)}`,
      `L ${svgNumber(CUSTOM_FIELD_OPTION_CONNECTOR_SPINE_OFFSET_REM)} ${svgNumber(centerY)}`,
    );
  }
  return parts.join(" ");
}
