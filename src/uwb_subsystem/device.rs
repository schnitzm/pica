use crate::uci_packets::{
    DeviceConfigId, DeviceConfigStatus, DeviceParameter, DeviceState, UciPacketPacket,
};
use crate::uwb_subsystem::*;
use std::collections::HashMap;
use std::iter::Extend;

use crate::position::Position;
use anyhow::Result;
use tokio::sync::mpsc;

use num_traits::FromPrimitive;

use super::session::{Session, MAX_SESSION};

pub const MAX_DEVICE: usize = 4;
const UCI_VERSION: u16 = 0x110; // Version 1.1.0
const MAC_VERSION: u16 = 0x130; // Version 1.3.0
const PHY_VERSION: u16 = 0x130; // Version 1.3.0
const TEST_VERSION: u16 = 0x110; // Version 1.1

// Capabilities are vendor defined, Android parses capabilities
// according to these definitions:
// /android/packages/modules/Uwb/service/java/com/android/server/uwb/config/CapabilityParam.java
pub const DEFAULT_CAPS_INFO: &'static [(CapTlvType, &'static [u8])] = &[
    (CapTlvType::SupportedFiraPhyVersionRange, &[1, 1, 1, 3]), // 1.1 - 1.3
    (CapTlvType::SupportedFiraMacVersionRange, &[1, 1, 1, 3]), // 1.1 - 1.3
    (CapTlvType::SupportedDeviceRoles, &[0x3]),                // INTIATOR | RESPONDER
    (CapTlvType::SupportedRangingMethod, &[0x1f]), // DS_TWR_NON_DEFERRED | SS_TWR_NON_DEFERRED | DS_TWR_DEFERRED | SS_TWR_DEFERRED | OWR
    (CapTlvType::SupportedStsConfig, &[0x7]), // STATIC_STS | DYNAMIC_STS | DYNAMIC_STS_RESPONDER_SPECIFIC_SUBSESSION_KEY
    (CapTlvType::SupportedMultiNodeModes, &[0x0]),
    (CapTlvType::SupportedBlockStriding, &[0x0]),
    (CapTlvType::SupportedUwbInitiationTime, &[0x0]),
    (CapTlvType::SupportedChannels, &[0xff]),
    (CapTlvType::SupportedRframeConfig, &[0x0]),
    (CapTlvType::SupportedBprfParameterSets, &[0x0]),
    (CapTlvType::SupportedHprfParameterSets, &[0x0]),
    (CapTlvType::SupportedCcConstraintLength, &[0x0]),
    (CapTlvType::SupportedAoa, &[0x0]),
    (CapTlvType::SupportedAoaResultReqAntennaInterleaving, &[0x0]),
    (CapTlvType::SupportedExtendedMacAddress, &[0x0]),
];

pub struct Device {
    pub mac_address: usize,
    pub position: Position,
    pub state: DeviceState,
    sessions: HashMap<u32, Session>,
    pub tx: mpsc::Sender<UciPacketPacket>,
    pub config: HashMap<u8, Vec<u8>>,
    pub country_code: [u8; 2],
}

impl Device {
    pub fn new(device_handle: usize, tx: mpsc::Sender<UciPacketPacket>) -> Self {
        Device {
            mac_address: device_handle,
            position: Position::default(),
            state: DeviceState::DeviceStateReady,
            sessions: Default::default(),
            tx,
            config: HashMap::new(),
            country_code: Default::default(),
        }
    }

    pub fn add_session(&mut self, session: Session) -> StatusCode {
        if self.sessions.len() > MAX_SESSION {
            return StatusCode::UciStatusMaxSesssionsExceeded;
        }
        match self.sessions.insert(session.id, session) {
            Some(_) => StatusCode::UciStatusSesssionDuplicate,
            None => StatusCode::UciStatusOk,
        }
    }

    pub async fn send_device_status_notification(&self, device_state: DeviceState) -> Result<()> {
        self.tx
            .send(DeviceStatusNtfBuilder { device_state }.build().into())
            .await?;
        Ok(())
    }

    pub async fn send_session_status_notification(
        &self,
        session_id: u32,
        session_state: SessionState,
        reason_code: ReasonCode,
    ) -> Result<()> {
        self.tx
            .send(
                SessionStatusNtfBuilder {
                    session_id,
                    session_state,
                    reason_code,
                }
                .build()
                .into(),
            )
            .await?;
        Ok(())
    }

    pub fn remove_session(&mut self, session_id: u32) -> Result<()> {
        if let Some(_) = self.sessions.remove(&session_id) {
            Ok(())
        } else {
            Err(anyhow!("Could not find session"))
        }
    }

    pub fn get_session(&self, session_id: u32) -> Option<&Session> {
        self.sessions.get(&session_id)
    }

    pub fn get_session_mut(&mut self, session_id: u32) -> Option<&mut Session> {
        self.sessions.get_mut(&session_id)
    }

    pub fn get_session_cnt(&self) -> usize {
        self.sessions.len()
    }
}

impl Pica {
    // The fira norm specify to send a response, then reset, then
    // send a notification once the reset is done
    pub async fn device_reset(
        &mut self,
        device_handle: usize,
        cmd: DeviceResetCmdPacket,
    ) -> Result<()> {
        let reset_config = cmd.get_reset_config();
        println!("[{}] DeviceReset", device_handle);
        println!("  reset_config={}", reset_config);
        {
            let mut device = self.get_device_mut(device_handle);
            let status = match reset_config {
                ResetConfig::UwbsReset => StatusCode::UciStatusOk,
            };
            device.state = DeviceState::DeviceStateReady;
            device
                .tx
                .send(DeviceResetRspBuilder { status }.build().into())
                .await?;
        }

        self.devices.insert(
            device_handle,
            Device::new(device_handle, self.devices[&device_handle].tx.clone()),
        );
        Ok(self
            .get_device(device_handle)
            .tx
            .send(
                DeviceStatusNtfBuilder {
                    device_state: DeviceState::DeviceStateReady,
                }
                .build()
                .into(),
            )
            .await?)
    }

    pub async fn get_device_info(
        &mut self,
        device_handle: usize,
        _cmd: GetDeviceInfoCmdPacket,
    ) -> Result<()> {
        // TODO: Implement a fancy build time state machine instead of crash at runtime
        println!("[{}] GetDeviceInfo", device_handle);
        let device = self.get_device(device_handle);
        assert_eq!(device.state, DeviceState::DeviceStateReady);
        Ok(device
            .tx
            .send(
                GetDeviceInfoRspBuilder {
                    status: StatusCode::UciStatusOk,
                    uci_version: UCI_VERSION,
                    mac_version: MAC_VERSION,
                    phy_version: PHY_VERSION,
                    uci_test_version: TEST_VERSION,
                    vendor_spec_info: Vec::new(),
                }
                .build()
                .into(),
            )
            .await?)
    }

    pub async fn get_caps_info(
        &mut self,
        device_handle: usize,
        cmd: GetCapsInfoCmdPacket,
    ) -> Result<()> {
        println!("[{}] GetCapsInfo", device_handle);
        assert_eq!(
            cmd.get_packet_boundary_flag(),
            PacketBoundaryFlag::Complete,
            "Boundary flag is true, implement fragmentation"
        );

        let caps = DEFAULT_CAPS_INFO
            .iter()
            .map(|(id, value)| CapTlv {
                t: *id,
                v: (*value).into(),
            })
            .collect();
        self.get_device(device_handle)
            .tx
            .send(
                GetCapsInfoRspBuilder {
                    status: StatusCode::UciStatusOk,
                    tlvs: caps,
                }
                .build()
                .into(),
            )
            .await?;
        Ok(())
    }

    pub async fn set_config(
        &mut self,
        device_handle: usize,
        cmd: SetConfigCmdPacket,
    ) -> Result<()> {
        println!("[{}] SetConfig", device_handle);
        let device = self.get_device_mut(device_handle);
        assert_eq!(device.state, DeviceState::DeviceStateReady); // UCI 6.3
        assert_eq!(
            cmd.get_packet_boundary_flag(),
            PacketBoundaryFlag::Complete,
            "Boundary flag is true, implement fragmentation"
        );

        let (valid_parameters, invalid_config_status) = cmd.get_parameters().iter().fold(
            (HashMap::new(), Vec::new()),
            |(mut valid_parameters, mut invalid_config_status), param| {
                let id = param.id;
                match DeviceConfigId::from_u8(id) {
                    Some(_) => {
                        valid_parameters.insert(param.id, param.value.clone());
                    }
                    None => invalid_config_status.push(DeviceConfigStatus {
                        parameter_id: id,
                        status: StatusCode::UciStatusInvalidParam,
                    }),
                };
                (valid_parameters, invalid_config_status)
            },
        );

        let (status, parameters) = if invalid_config_status.is_empty() {
            device.config.extend(valid_parameters.into_iter());
            (StatusCode::UciStatusOk, Vec::new())
        } else {
            (StatusCode::UciStatusInvalidParam, invalid_config_status)
        };

        Ok(device
            .tx
            .send(SetConfigRspBuilder { status, parameters }.build().into())
            .await?)
    }

    pub async fn get_config(
        &mut self,
        device_handle: usize,
        cmd: GetConfigCmdPacket,
    ) -> Result<()> {
        println!("[{}] GetConfig", device_handle);
        assert_eq!(
            cmd.get_packet_boundary_flag(),
            PacketBoundaryFlag::Complete,
            "Boundary flag is true, implement fragmentation"
        );
        let device = self.get_device(device_handle);
        let ids = cmd.get_parameter_ids();

        let (valid_parameters, invalid_parameters) = ids.iter().fold(
            (Vec::new(), Vec::new()),
            |(mut valid_parameters, mut invalid_parameters), id| {
                // UCI Core Section 6.3.2 Table 8
                // UCI Core Section 6.3.2 - Return the Configuration
                // If the status code is ok, return the params
                // If there is at least one invalid param, return the list of invalid params
                // If the ID is not present in our config, return the Type with length = 0
                match device.config.get(id) {
                    Some(value) => valid_parameters.push(DeviceParameter {
                        id: *id,
                        value: value.clone(),
                    }),
                    None => invalid_parameters.push(DeviceParameter {
                        id: *id,
                        value: Vec::new(),
                    }),
                }

                (valid_parameters, invalid_parameters)
            },
        );

        let (status, parameters) = if invalid_parameters.is_empty() {
            (StatusCode::UciStatusOk, valid_parameters)
        } else {
            (StatusCode::UciStatusInvalidParam, invalid_parameters)
        };

        Ok(device
            .tx
            .send(GetConfigRspBuilder { status, parameters }.build().into())
            .await?)
    }
}
