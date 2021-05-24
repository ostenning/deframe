//! # deframe
//! A data deframer for embedded and no_std applications

#![no_std]
#![deny(warnings)]
#![allow(dead_code)]

pub struct Deframer<const N: usize> {
    remainder: [u8; N],
    remainder_length: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DeframeError {
    Overflow,
}

impl<const N: usize> Deframer<N> {
    pub fn new() -> Self {
        Self {
            remainder: [0; N],
            remainder_length: 0,
        }
    }

    pub fn deframe(&mut self, data_frame: &[u8], get_frame_end: fn(iter: &mut core::slice::Iter<u8>) -> Option<usize>) -> Result<([u8; N], usize), DeframeError> {
        // check if the last read had some dangling/remainding bytes after the last linebreak
        let with_remainder = self.remainder_length != 0;

        if !with_remainder {
            let frame_end_result =  get_frame_end(&mut data_frame.iter());
            let frame_end_pos = if frame_end_result.is_some() { 
                frame_end_result.unwrap() + 1
            } else {
                // If no frame end is found, then all the data is reserve data
                0
            };
        
            self.remainder_length = data_frame.len() - frame_end_pos;
            self.remainder[0..self.remainder_length]
                .copy_from_slice(&data_frame[frame_end_pos..data_frame.len()]);

            let mut data: [u8; N] = [0; N];
            if frame_end_pos > N {
                return Err(DeframeError::Overflow);
            }
            data[0..frame_end_pos].copy_from_slice(&data_frame[0..frame_end_pos]);
            return Ok((data, frame_end_pos));
        }

        // Keep finding the next valid position until our data is within the buffer size
        let mut iter = data_frame.iter();
        let mut last_valid_pos: usize = N;
        while last_valid_pos + self.remainder_length > N {
            let frame_end_result = get_frame_end(&mut iter);
            if frame_end_result.is_some() {
                last_valid_pos = frame_end_result.unwrap() + 1;
            } else {
                last_valid_pos = 0;
                break;
            }
        }

        // No frame break was found, in this case all the current data must be pushed to the
        // remainder (for the next deframe call) and no data returned to the user
        if last_valid_pos == 0 {
            if data_frame.len() + self.remainder_length > N {
                return Err(DeframeError::Overflow);
            }
            self.remainder[self.remainder_length..data_frame.len() + self.remainder_length].copy_from_slice(&data_frame[0..data_frame.len()]);
            self.remainder_length = data_frame.len() + self.remainder_length;
            return Ok(([0; N], 0));
        }

        // If there is remainding line data from the previous reader, prepend it
        let mut appended: [u8; N] = [0; N];
        appended[0..self.remainder_length]
            .copy_from_slice(&self.remainder[0..self.remainder_length]);

        let end_pos = self.remainder_length + last_valid_pos;

        if end_pos > N {
            return Err(DeframeError::Overflow);
        }

        // [remainder...trimmed_data]
        appended[self.remainder_length..end_pos].copy_from_slice(&data_frame[0..last_valid_pos]);

        self.remainder_length = data_frame.len() - last_valid_pos;
        
        if self.remainder_length > N {
            return Err(DeframeError::Overflow);
        }
        self.remainder[0..self.remainder_length].copy_from_slice(&data_frame[last_valid_pos..data_frame.len()]);
       
        // This data should be valid for the CSV parser
        return Ok((appended, end_pos));
    }
}

#[cfg(test)]
mod tests {
    use core::slice::Iter;

    use crate::{DeframeError, Deframer};

    /// The frame end, which is an ASCII linebreak for these tests
    const FRAME_END: u8 = 0x0A;
    /// For these tests, we simply denote a frame by an ASCII line-break (similar to CSV)
    const GET_FRAME_END: fn(iter: &mut Iter<u8>) -> Option<usize> = |iter| iter.rposition(|&x| x == FRAME_END);

    #[test]
    fn finds_the_correct_frame_end() {
        let mut deframer = Deframer::<4>::new();
        let (result, len) = deframer.deframe(&[FRAME_END, 0x01, 0x02, 0x03], GET_FRAME_END).unwrap();
        assert_eq!(result[0..len], [FRAME_END]);

        let mut deframer = Deframer::<4>::new();
        let (result, len) = deframer.deframe(&[0x01, FRAME_END, 0x02, 0x03], GET_FRAME_END).unwrap();
        assert_eq!(len, 2);
        assert_eq!(result[0..len], [0x01, FRAME_END]);

        let mut deframer = Deframer::<4>::new();
        let (result, len) = deframer.deframe(&[0x01, 0x02, 0x03, FRAME_END], GET_FRAME_END).unwrap();
        assert_eq!(len, 4);
        assert_eq!(result[0..len], [0x01, 0x02, 0x03, FRAME_END]);
    }

    #[test]
    fn has_the_correct_remainder() {
        let mut deframer = Deframer::<16>::new();
        let (result, len) = deframer.deframe(&[FRAME_END, 0x01, 0x02, 0x03], GET_FRAME_END).unwrap();
        assert_eq!(result[0..len], [FRAME_END]);
        assert_eq!(deframer.remainder_length, 3);

        let (result, len) = deframer.deframe(&[0x04, 0x05, FRAME_END, 0x06], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 1);
        assert_eq!(deframer.remainder[0..deframer.remainder_length], [0x06]);
        assert_eq!(result[0..len], [0x01, 0x02, 0x03, 0x04, 0x05, FRAME_END]);

        let (result, len) = deframer.deframe(&[0x07, 0x08, 0x09, 0x10, FRAME_END, 0x11, 0x22], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 2);
        assert_eq!(deframer.remainder[0..deframer.remainder_length], [0x11, 0x22]);
        assert_eq!(result[0..len], [0x06, 0x07, 0x08, 0x09, 0x10, FRAME_END]);
    }

    #[test]
    fn correctly_overflows() {
        let mut deframer = Deframer::<2>::new();
        
        // Overflows because the data_frame provider is too large for the allocated buffer
        let result = deframer.deframe(&[0x01, 0x02, 0x03, FRAME_END], GET_FRAME_END);
        assert_eq!(result.is_err(), true);

        let mut deframer = Deframer::<2>::new();
        
        let result = deframer.deframe(&[0x01], GET_FRAME_END);
        assert_eq!(result.is_err(), false);
        assert_eq!(deframer.remainder_length, 1);
        
        let result = deframer.deframe(&[0x02], GET_FRAME_END);
        assert_eq!(result.is_err(), false);
        assert_eq!(deframer.remainder_length, 2);
        
        let result = deframer.deframe(&[0x03], GET_FRAME_END);
        assert_eq!(result.is_err(), true);
        assert_eq!(result.err().unwrap(), DeframeError::Overflow);
    }

    #[test]
    fn remainder_increases() {
        let mut deframer = Deframer::<4>::new();
      
        let (_data, len) = deframer.deframe(&[0x01], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 1);
        assert_eq!(len, 0);
      
        let (_data, len) = deframer.deframe(&[0x02], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 2);
        assert_eq!(len, 0);
      
        let (_data, len) = deframer.deframe(&[0x03], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 3);
        assert_eq!(len, 0);
      
        let (data, len) = deframer.deframe(&[FRAME_END], GET_FRAME_END).unwrap();
        assert_eq!(deframer.remainder_length, 0);
        assert_eq!(len, 4);
        assert_eq!(data, [0x01, 0x02, 0x03, FRAME_END]);
    }
}   