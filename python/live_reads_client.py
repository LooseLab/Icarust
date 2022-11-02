"""Test simple read until functionality"""

import numpy

import read_until
from minknow_api import data_pb2
import time
import logging
from rich.logging import RichHandler


logger = logging.Logger("")
logger.addHandler(RichHandler())

def test_response():
    """Test client response"""
    try:
        client = read_until.ReadUntilClient(mk_host="127.0.0.1", mk_port=10001)
        client_iteration = 0
        client.run(first_channel=1, last_channel=3000)
        time.sleep(0.4)
        while client.is_running:
            for read_number, read_chunk in client.get_read_chunks(batch_size=24, last=True):
                logger.info(f"{client_iteration}, {read_chunk.id}")
            client_iteration += 1
    finally:
        logger.critical("Client closing connection")
        client.reset()
        




if __name__ == "__main__":
    test_response()
