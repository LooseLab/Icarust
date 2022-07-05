from ont_fast5_api.multi_fast5 import MultiFast5File
from uuid import uuid4
import numpy as np

if __name__ == "__main__":
    context_tags = {
        "barcoding_enabled": "0",
        "experiment_duration_set": "4800",
        "experiment_type": "genomic_dna",
        "local_basecalling": "0",
        "package": "bream4",
        "package_version": "6.3.5",
        "sample_frequency": "4000",
        "sequencing_kit": "sqk-lsk109",
    }
    tracking_id = {
        "asic_id": "817405089",
        "asic_id_eeprom": "5661715",
        "asic_temp": "29.357218",
        "asic_version": "IA02D",
        "auto_update": "0",
        "auto_update_source": "https://mirror.oxfordnanoportal.com/software/MinKNOW/",
        "bream_is_standard": "0",
        "configuration_version": "4.4.13",
        "device_id": "X420",
        "device_type": "gridion",
        "distribution_status": "stable",
        "distribution_version": "21.10.8",
        "exp_script_name": "sequencing/sequencing_MIN106_DNA:FLO-MIN106:SQK-LSK109",
        "exp_script_purpose": "sequencing_run",
        "exp_start_time": '2021-12-17T16:54:04.325472+00:00',
        "flow_cell_id": 'TEST0001',
        "flow_cell_product_code": "FLO-MIN106",
        "guppy_version": "5.0.17+99baa5b",
        "heatsink_temp": "34.066406",
        "host_product_code": "GRD-X5B003",
        "host_product_serial_number": "NOTFOUND",
        "hostname": "master",
        "installation_type": "nc",
        "local_firmware_file": "1",
        "operating_system": "ubuntu 16.04",
        "protocol_group_id": "testing_fast5",
        "protocol_run_id": "SYNTHETIC_RUN",
        "protocol_start_time":'2021-12-17T16:49:35.705717+00:00',
        "protocols_version": "6.3.5",
        "run_id": str(uuid4()),
        "sample_id": "simple sample",
        "usb_config": "fx3_1.2.4#fpga_1.2.1#bulk#USB300",
        "version": "4.4.3",
    }

    file_name = "test_barcoding.fast5"
    reads_to_add = range(5)
    with MultiFast5File(
        file_name, "a", driver="core"
    ) as multi_f5:  # just opening this file is slow so only do it when there are reads to write
        # while self.read_set:
        for read_to_add in reads_to_add:
            read_bytes = np.load("barcoding/squiggle/Barcode01_1.squiggle.npy")
            # read_bytes = np.concat((read_bytes, pad_bytes))
            read0 = multi_f5.create_empty_read(
                str(uuid4()), "str(self.run_id)"
            )
            raw_attrs = {
                "duration": read_bytes.size,
                "median_before": 2.5,
                "read_id": str(uuid4()),
                "read_number": read_to_add,
                "start_mux": 1,
                "start_time": 400,#note this is the start time of the read in samples
                "end_reason": 0,
            }
            read0.add_raw_data(read_bytes, attrs=raw_attrs)
            channel_info = {
                "digitisation": 8192.0,
                "offset": 6.0,
                "range": 1500.0,
                "sampling_rate": 4000.0,
                "channel_number": 1,
            }
            read0.add_channel_info(channel_info)
            read0.add_tracking_id(tracking_id)
            read0.add_context_tags(context_tags)