//! State structs.
#![deny(missing_docs)]

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use vipers::prelude::*;
use vipers::program_err;

/// A [SmartWallet] is a multisig wallet with Timelock capabilities.
#[account]
#[derive(Default, Debug, PartialEq)]
#[repr(C)]
pub struct SmartWallet {
    /// Base used to derive.
    pub base: Pubkey,
    /// Bump seed for deriving PDA seeds.
    pub bump: u8,

    /// Minimum number of owner approvals needed to sign a [Transaction].
    pub threshold: u64,
    /// Minimum delay between approval and execution, in seconds.
    pub minimum_delay: i64,
    /// Time after the ETA until a [Transaction] expires.
    pub grace_period: i64,

    /// Sequence of the ownership set.
    ///
    /// This may be used to see if the owners on the multisig have changed
    /// since the last time the owners were checked. This is used on
    /// [Transaction] approval to ensure that owners cannot approve old
    /// transactions.
    pub owner_set_seqno: u32,
    /// Total number of [Transaction]s on this [SmartWallet].
    pub num_transactions: u64,

    /// Owners of the [SmartWallet].
    pub owners: Vec<Pubkey>,

    /// Extra space for program upgrades.
    pub reserved: [u64; 16],
}

impl SmartWallet {
    /// Computes the space a [SmartWallet] uses.
    pub fn space(max_owners: u8) -> usize {
        4 // Anchor discriminator
            + std::mem::size_of::<SmartWallet>()
            + 4 // 4 = the Vec discriminator
            + std::mem::size_of::<Pubkey>() * (max_owners as usize)
    }

    /// Gets the index of the key in the owners Vec, or None
    pub fn owner_index_opt(&self, key: Pubkey) -> Option<usize> {
        self.owners.iter().position(|a| *a == key)
    }

    /// Gets the index of the key in the owners Vec, or error
    pub fn try_owner_index(&self, key: Pubkey) -> Result<usize> {
        Ok(unwrap_opt!(self.owner_index_opt(key), InvalidOwner))
    }
}

/// A [Transaction] is a series of instructions that may be executed
/// by a [SmartWallet].
#[account]
#[derive(Debug, Default, PartialEq)]
#[repr(C)]
pub struct Transaction {
    /// The [SmartWallet] account this transaction belongs to.
    pub smart_wallet: Pubkey,
    /// The auto-incremented integer index of the transaction.
    /// All transactions on the [SmartWallet] can be looked up via this index,
    /// allowing for easier browsing of a wallet's historical transactions.
    pub index: u64,
    /// Bump seed.
    pub bump: u8,

    /// The proposer of the [Transaction].
    pub proposer: Pubkey,
    /// The instruction.
    pub instructions: Vec<TXInstruction>,
    /// `signers[index]` is true iff `[SmartWallet]::owners[index]` signed the transaction.
    pub signers: Vec<bool>,
    /// Owner set sequence number.
    pub owner_set_seqno: u32,
    /// Estimated time the [Transaction] will be executed.
    ///
    /// - If set to [crate::NO_ETA], the transaction may be executed at any time.
    /// - Otherwise, the [Transaction] may be executed at any point after the ETA has elapsed.
    pub eta: i64,

    /// The account that executed the [Transaction].
    pub executor: Pubkey,
    /// When the transaction was executed. -1 if not executed.
    pub executed_at: i64,
}

impl Transaction {
    /// Computes the space a [Transaction] uses.
    pub fn space(instructions: Vec<TXInstruction>) -> usize {
        4  // Anchor discriminator
            + std::mem::size_of::<Transaction>()
            + 4 // Vec discriminator
            + (instructions.iter().map(|ix| ix.space()).sum::<usize>())
    }

    /// Number of signers.
    pub fn num_signers(&self) -> usize {
        self.signers.iter().filter(|&did_sign| *did_sign).count()
    }
}

/// Instruction.
#[derive(AnchorSerialize, AnchorDeserialize, Default, Clone, Debug, PartialEq)]
#[repr(C)]
pub struct TXInstruction {
    /// Pubkey of the instruction processor that executes this instruction
    pub program_id: Pubkey,
    /// Metadata for what accounts should be passed to the instruction processor
    pub keys: Vec<TXAccountMeta>,
    /// Opaque data passed to the instruction processor
    pub data: Vec<u8>,
    /// Additional addresses that sign for things for a [SmartWallet]
    pub partial_signers: Vec<PartialSigner>
}

impl TXInstruction {
    /// Space that a [TXInstruction] takes up.
    pub fn space(&self) -> usize {
        std::mem::size_of::<Pubkey>()
            + (self.keys.len() as usize) * std::mem::size_of::<TXAccountMeta>()
            + (self.data.len() as usize)
            + (self.partial_signers.len() as usize) * std::mem::size_of::<PartialSigner>()
    }
}

/// Account metadata used to define [TXInstruction]s
#[derive(AnchorSerialize, AnchorDeserialize, Default, Copy, Clone, Debug, PartialEq)]
#[repr(C)]
pub struct TXAccountMeta {
    /// An account's public key
    pub pubkey: Pubkey,
    /// True if an Instruction requires a Transaction signature matching `pubkey`.
    pub is_signer: bool,
    /// True if the `pubkey` can be loaded as a read-write account.
    pub is_writable: bool,
}

impl From<&TXInstruction> for solana_program::instruction::Instruction {
    fn from(tx: &TXInstruction) -> solana_program::instruction::Instruction {
        solana_program::instruction::Instruction {
            program_id: tx.program_id,
            accounts: tx.keys.clone().into_iter().map(Into::into).collect(),
            data: tx.data.clone(),
        }
    }
}

impl From<TXAccountMeta> for solana_program::instruction::AccountMeta {
    fn from(
        TXAccountMeta {
            pubkey,
            is_signer,
            is_writable,
        }: TXAccountMeta,
    ) -> solana_program::instruction::AccountMeta {
        solana_program::instruction::AccountMeta {
            pubkey,
            is_signer,
            is_writable,
        }
    }
}

/// A partial signer that signs things for a [SmartWallet]
#[derive(AnchorSerialize, AnchorDeserialize, Default, Copy, Clone, Debug, PartialEq)]
#[repr(C)]
pub struct PartialSigner {
    /// The partial signer index seed
    pub index: u64,
    /// The partial signer bump seed
    pub bump: u8,
}

/// Type of Subaccount.
#[derive(
    AnchorSerialize, AnchorDeserialize, Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord,
)]
#[repr(C)]
pub enum SubaccountType {
    /// Requires the normal multisig approval process.
    Derived = 0,
    /// Any owner may sign an instruction  as this address.
    OwnerInvoker = 1,
}

impl Default for SubaccountType {
    fn default() -> Self {
        SubaccountType::Derived
    }
}

/// Mapping of a Subaccount to its [SmartWallet].
#[account]
#[derive(Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SubaccountInfo {
    /// Smart wallet of the sub-account.
    pub smart_wallet: Pubkey,
    /// Type of sub-account.
    pub subaccount_type: SubaccountType,
    /// Index of the sub-account.
    pub index: u64,
}

impl SubaccountInfo {
    /// Number of bytes that a [SubaccountInfo] uses.
    pub const LEN: usize = 32 + 1 + 8;
}

/// An account which holds an array of TxInstructions to be executed.
#[account]
#[derive(Default, Debug, PartialEq)]
#[repr(C)]
pub struct InstructionBuffer {
    /// Sequence of the ownership set.
    ///
    /// This may be used to see if the owners on the multisig have changed
    pub owner_set_seqno: u32,
    /// - If set to [crate::NO_ETA], the instructions in each [InstructionBuffer::bundles] may be executed at any time.
    /// - Otherwise, instructions may be executed at any point after the ETA has elapsed.
    pub eta: i64,
    /// Authority of the buffer.
    pub authority: Pubkey,
    /// Role that can execute instructions off the buffer.
    pub executor: Pubkey,
    /// Smart wallet the buffer belongs to.
    pub smart_wallet: Pubkey,
    /// Vector of instructions.
    pub bundles: Vec<InstructionBundle>,
}

impl InstructionBuffer {
    /// Check if the [InstructionBuffer] has been finalized.
    pub fn is_finalized(&self) -> bool {
        self.authority == Pubkey::default()
    }

    /// Get the [InstructionBundle] at the specified bundle index.
    pub fn get_bundle(&self, bundle_index: usize) -> Option<InstructionBundle> {
        let (_, right) = self.bundles.split_at(bundle_index);
        if right.is_empty() {
            None
        } else {
            Some(right[0].clone())
        }
    }

    /// Set the [InstructionBundle] at the specified bundle index.
    pub fn set_bundle(
        &mut self,
        bundle_index: usize,
        new_bundle: &InstructionBundle,
    ) -> Result<()> {
        let bundles = &mut self.bundles;

        if let Some(mut_bundle_ref) = bundles.get_mut(bundle_index) {
            *mut_bundle_ref = new_bundle.clone();
        } else if bundle_index == bundles.len() {
            bundles.push(new_bundle.clone())
        } else {
            return program_err!(BufferBundleNotFound);
        }

        Ok(())
    }
}

/// Container holding a bundle of instructions.
#[derive(AnchorSerialize, AnchorDeserialize, Default, Clone, Debug, PartialEq)]
#[repr(C)]
pub struct InstructionBundle {
    /// Execution counter on the [InstructionBundle].
    pub is_executed: bool,
    /// Vector of [TXInstruction] to be executed.
    pub instructions: Vec<TXInstruction>,
}
