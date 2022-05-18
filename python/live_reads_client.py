"""Test simple read until functionality"""

import numpy

import read_until
from minknow_api import data_pb2
import time
import logging
from rich.logging import RichHandler


logger = logging.Logger("")
logger.setLevel(logging.DEBUG)
logger.addHandler(RichHandler)

def test_response():
    """Test client response"""

    client = read_until.ReadUntilClient(mk_host="127.0.0.1", mk_port=10001)

    try:
        client.run(first_channel=4, last_channel=100)

        read_count = 0
        time.sleep(0.4)
        logger.info(list(client.get_read_chunks()))
        print(read_count)
        assert read_count == 1

    finally:
        client.reset()


if __name__ == "__main__":
    test_response()
